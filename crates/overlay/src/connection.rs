//! Owns only our attachment helper. The game process is never terminated by the overlay.
use dd_protocol::{AdapterStatus, Command, Request};
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command as ProcessCommand, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct GameConnection {
    child: Child,
    pub pid: u32,
}

impl GameConnection {
    pub fn start() -> Result<(Self, AdapterStatus), String> {
        use std::os::windows::process::CommandExt;
        let processes = crate::game::process_ids();
        let [pid] = processes.as_slice() else {
            return Err(
                "Start exactly one Dungeon Defenders instance in local single-player.".into(),
            );
        };
        let path = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("dd-adapter-host.exe");
        let child = ProcessCommand::new(path)
            .args(["--attach", &pid.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|e| {
                format!(
                    "Could not start the attachment helper. Keep all release files together. {e}"
                )
            })?;
        let mut connection = Self { child, pid: *pid };
        let mut line = String::new();
        let output = connection
            .child
            .stdout
            .take()
            .ok_or("Attachment helper output is unavailable.")?;
        BufReader::new(output)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        if line.trim() != format!("Attached to game process {pid}.") {
            let mut error = String::new();
            if let Some(mut stderr) = connection.child.stderr.take() {
                let _ = stderr.read_to_string(&mut error);
            }
            return Err(format!("Game attachment failed. {}", error.trim()));
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if connection
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                let mut error = String::new();
                if let Some(mut stderr) = connection.child.stderr.take() {
                    let _ = stderr.read_to_string(&mut error);
                }
                return Err(format!("Game attachment failed. {}", error.trim()));
            }
            match request(*pid, Command::Status {}) {
                Ok(status) => return Ok((connection, status)),
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for GameConnection {
    fn drop(&mut self) {
        // EOF asks only our helper to remove its thread hook; the pinned DLL becomes idle.
        self.child.stdin.take();
        let deadline = Instant::now() + Duration::from_millis(500);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

pub fn request(pid: u32, command: Command) -> Result<AdapterStatus, String> {
    let response = dd_protocol::pipe::request(pid, &Request::new(1, command)).map_err(|e| format!("Game adapter communication failed: {e}. If spawning was requested, check nearby pickups before retrying."))?;
    let status = response.result.map_err(|e| e.message)?;
    if status.pid != pid || status.bits != 32 || status.diagnostic {
        return Err("Unexpected game adapter identity.".into());
    }
    Ok(status)
}
