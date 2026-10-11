//! Diagnostic entry point and build-specific single-player game adapter.

#[cfg(windows)]
pub mod attachment;
#[cfg(windows)]
mod bridge;
#[cfg(windows)]
mod engine;

use dd_protocol::{
    AdapterStatus, Command, ErrorCode, Rejection, Request, Response, VERSION, validate_grant,
};

pub fn status() -> AdapterStatus {
    let diagnostic = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .is_some_and(|name| name.eq_ignore_ascii_case("dd-adapter-host.exe"));
    AdapterStatus {
        pid: std::process::id(),
        bits: usize::BITS,
        diagnostic,
        can_give: false,
        message: if diagnostic {
            "Isolated adapter test passed. Dungeon Defenders is not connected; item granting is unavailable."
        } else {
            "Diagnostic entry point only. Use the game attachment helper for engine requests."
        }
        .into(),
    }
}

pub fn handle(request: Request) -> Response {
    let rejection = |code, message: &str| {
        Err(Rejection {
            code,
            message: message.into(),
        })
    };
    let result = if request.version != VERSION {
        rejection(
            ErrorCode::ProtocolMismatch,
            "Overlay and adapter protocol versions differ.",
        )
    } else {
        match request.command {
            Command::Status {} | Command::Shutdown {} => Ok(status()),
            Command::Give { template, quantity } => match validate_grant(&template, quantity) {
                Err(message) => rejection(ErrorCode::InvalidRequest, message),
                Ok(()) => rejection(
                    ErrorCode::IntegrationUnavailable,
                    "Isolated diagnostics cannot spawn items. Use Connect to game from the overlay.",
                ),
            },
        }
    };
    Response {
        version: VERSION,
        id: request.id,
        result,
    }
}

#[cfg(windows)]
fn serve() -> std::io::Result<()> {
    use dd_protocol::{pipe::Pipe, read_frame, write_frame};
    use std::io::Read;
    let mut pipe = Pipe::bind(std::process::id())?;
    loop {
        pipe.accept()?;
        let stop = match read_frame::<Request>(&mut pipe) {
            Ok(request) => {
                let stop =
                    request.version == VERSION && matches!(request.command, Command::Shutdown {});
                let response = handle(request);
                if write_frame(&mut pipe, &response).is_ok() {
                    // Bound the wait for the client to consume the response; never FlushFileBuffers here.
                    let _ = pipe.read_exact(&mut [0]);
                }
                stop
            }
            Err(_) => false,
        };
        pipe.disconnect();
        if stop {
            return Ok(());
        }
    }
}

/// Runs the IPC service on the calling thread. Call after LoadLibrary has returned.
/// No engine calls, hooks, or work under DllMain's loader lock are performed.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn dd_adapter_run() -> u32 {
    match std::panic::catch_unwind(serve) {
        Ok(Ok(())) => 0,
        Ok(Err(_)) => 1,
        Err(_) => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unverified_backend_never_reports_success() {
        let response = handle(Request::new(
            7,
            Command::Give {
                template: "Package.Item".into(),
                quantity: 1,
            },
        ));
        assert_eq!(response.id, 7);
        assert_eq!(
            response.result.unwrap_err().code,
            ErrorCode::IntegrationUnavailable
        );
        assert!(!status().can_give);
    }

    #[test]
    fn rejects_version_before_dispatch() {
        let mut request = Request::new(8, Command::Status {});
        request.version = VERSION + 1;
        assert_eq!(
            handle(request).result.unwrap_err().code,
            ErrorCode::ProtocolMismatch
        );
    }

    #[test]
    fn rejects_invalid_grant() {
        let response = handle(Request::new(
            9,
            Command::Give {
                template: "A.B".into(),
                quantity: 0,
            },
        ));
        assert_eq!(response.result.unwrap_err().code, ErrorCode::InvalidRequest);
    }
}
