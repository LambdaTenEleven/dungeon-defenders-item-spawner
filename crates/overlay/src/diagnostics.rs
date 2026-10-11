use dd_protocol::{AdapterStatus, Command, ErrorCode, Request};
use std::{
    process::{Child, Command as ProcessCommand},
    thread,
    time::{Duration, Instant},
};

pub struct DiagnosticHost {
    child: Child,
}

impl DiagnosticHost {
    #[cfg(windows)]
    pub fn start() -> Result<(Self, AdapterStatus), String> {
        use std::os::windows::process::CommandExt;
        let path = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("dd-adapter-host.exe");
        let child = ProcessCommand::new(path)
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|e| {
                format!("Could not start the adapter test. Keep all release files together. {e}")
            })?;
        let mut host = Self { child };
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(code) = host.child.try_wait().map_err(|e| e.to_string())? {
                return Err(format!("Adapter test exited before connecting ({code})."));
            }
            match host.send(Request::new(1, Command::Status {})) {
                Ok(response) => {
                    let status = response.result.map_err(|e| e.message)?;
                    if !status.diagnostic
                        || status.bits != 32
                        || status.pid != host.child.id()
                        || status.can_give
                    {
                        return Err(
                            "The adapter test returned an unexpected identity or capability."
                                .into(),
                        );
                    }
                    return Ok((host, status));
                }
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => thread::sleep(Duration::from_millis(50)),
            }
        }
    }

    #[cfg(windows)]
    fn send(&self, request: Request) -> Result<dd_protocol::Response, String> {
        dd_protocol::pipe::request(self.child.id(), &request).map_err(|e| e.to_string())
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for DiagnosticHost {
    fn drop(&mut self) {
        // This is only our isolated diagnostic child, never the game process.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
pub fn self_test() -> Result<(), String> {
    let (mut host, status) = DiagnosticHost::start()?;
    println!(
        "PASS: loaded real {}-bit Rust adapter DLL in diagnostic host {}",
        status.bits, status.pid
    );
    let invalid = host.send(Request::new(
        2,
        Command::Give {
            template: "Bad;Command".into(),
            quantity: 1,
        },
    ))?;
    if !matches!(invalid.result, Err(ref e) if e.code == ErrorCode::InvalidRequest) {
        return Err("Invalid request was not rejected.".into());
    }
    println!("PASS: invalid item request rejected");
    let grant = host.send(Request::new(
        3,
        Command::Give {
            template: "Package.Item".into(),
            quantity: 1,
        },
    ))?;
    if !matches!(grant.result, Err(ref e) if e.code == ErrorCode::IntegrationUnavailable) {
        return Err("Unverified item grant did not fail closed.".into());
    }
    println!("PASS: valid item request rejected in the isolated diagnostic host");
    let mut incompatible = Request::new(4, Command::Status {});
    incompatible.version += 1;
    let response = host.send(incompatible)?;
    if !matches!(response.result, Err(ref e) if e.code == ErrorCode::ProtocolMismatch) {
        return Err("Protocol mismatch was not rejected.".into());
    }
    println!("PASS: incompatible protocol rejected");
    host.send(Request::new(5, Command::Shutdown {}))?
        .result
        .map_err(|e| e.message)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(code) = host.child.try_wait().map_err(|e| e.to_string())? {
            if !code.success() {
                return Err(format!("Adapter exit failed: {code}"));
            }
            println!("PASS: adapter shutdown and DLL unload completed");
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Adapter did not stop.".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}
