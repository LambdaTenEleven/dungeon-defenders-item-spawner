//! Owns only our attachment helper. The game process is never terminated by the overlay.
use dd_catalog::CatalogKey;
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
    pub catalog: CatalogKey,
}

impl GameConnection {
    pub fn start(catalog: CatalogKey) -> Result<(Self, AdapterStatus), String> {
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
        let mut connection = Self {
            child,
            pid: *pid,
            catalog,
        };
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
                Ok(_) => {
                    let status = request(
                        *pid,
                        Command::BindCatalog {
                            catalog: connection.catalog.clone(),
                        },
                    )?;
                    return Ok((connection, status));
                }
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
    let catalog = match &command {
        Command::BindCatalog { catalog }
        | Command::InspectItem { catalog, .. }
        | Command::Spawn { catalog, .. } => Some(catalog.clone()),
        Command::Status {} | Command::Shutdown {} => None,
    };
    let response = dd_protocol::pipe::request(pid, &Request::new(1, command)).map_err(|e| format!("Game adapter communication failed: {e}. If spawning was requested, check nearby pickups before retrying."))?;
    let status = response.result.map_err(|error| {
        if let Some(outcome) = error.spawn {
            let uncertainty = if outcome.uncertain {
                " An additional dispatched pickup may exist; check near your hero before retrying."
            } else {
                ""
            };
            format!(
                "{}/{} pickups confirmed. {}{uncertainty}",
                outcome.spawned, outcome.requested, error.message
            )
        } else {
            error.message
        }
    })?;
    validate_status(pid, catalog.as_ref(), &status)?;
    Ok(status)
}

fn validate_status(
    pid: u32,
    catalog: Option<&CatalogKey>,
    status: &AdapterStatus,
) -> Result<(), String> {
    if status.pid != pid || status.bits != 32 || status.diagnostic {
        return Err("Unexpected game adapter identity.".into());
    }
    if catalog.is_some() && status.catalog.as_ref() != catalog {
        return Err("Game adapter returned a different catalog generation. Reconnect before another request; check nearby pickups if a spawn was dispatched.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(catalog: Option<CatalogKey>) -> AdapterStatus {
        AdapterStatus {
            pid: 1,
            catalog,
            bits: 32,
            diagnostic: false,
            can_give: true,
            roll_matched: None,
            message: String::new(),
            inspection: None,
            spawn: None,
        }
    }

    #[test]
    fn item_responses_require_the_bound_installation_and_generation() {
        let expected = CatalogKey {
            installation_id: "a".repeat(64),
            generation: "b".repeat(64),
        };
        assert!(validate_status(1, Some(&expected), &status(Some(expected.clone()))).is_ok());
        assert!(validate_status(1, Some(&expected), &status(None)).is_err());
        let other_installation = CatalogKey {
            installation_id: "c".repeat(64),
            ..expected.clone()
        };
        assert!(validate_status(1, Some(&expected), &status(Some(other_installation))).is_err());
        let other_generation = CatalogKey {
            generation: "c".repeat(64),
            ..expected.clone()
        };
        assert!(validate_status(1, Some(&expected), &status(Some(other_generation))).is_err());
    }

    #[test]
    fn unbound_status_probe_still_requires_the_correct_game_adapter() {
        assert!(validate_status(1, None, &status(None)).is_ok());
        assert!(validate_status(2, None, &status(None)).is_err());
        let mut diagnostic = status(None);
        diagnostic.diagnostic = true;
        assert!(validate_status(1, None, &diagnostic).is_err());
    }
}
