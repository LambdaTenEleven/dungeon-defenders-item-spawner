//! Local named-pipe transport. Each connection carries one request and response.

use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING,
        INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile},
    System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
        PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        SetNamedPipeHandleState, WaitNamedPipeW,
    },
};

use crate::{MAX_FRAME_SIZE, Request, Response, VERSION, read_frame, write_frame};

const TIMEOUT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(10);

fn pipe_name(pid: u32) -> Vec<u16> {
    format!(r"\\.\pipe\dungeon-defenders-overlay-{pid}")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

/// A pipe with a deadline for the complete request/response, not per byte.
pub struct Pipe {
    file: File,
    deadline: Instant,
}

impl Pipe {
    fn wait(&self) -> io::Result<()> {
        self.check_deadline()?;
        thread::sleep(POLL);
        Ok(())
    }

    fn check_deadline(&self) -> io::Result<()> {
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Adapter did not respond within two seconds.",
            ));
        }
        Ok(())
    }

    pub fn bind(pid: u32) -> io::Result<Self> {
        let name = pipe_name(pid);
        // SAFETY: name is NUL terminated; the returned handle is checked and owned below.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                MAX_FRAME_SIZE as u32,
                MAX_FRAME_SIZE as u32,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateNamedPipeW transferred a valid, uniquely owned handle to us.
        let file = unsafe { File::from_raw_handle(handle) };
        Ok(Self {
            file,
            deadline: Instant::now() + TIMEOUT,
        })
    }

    pub fn accept(&mut self) -> io::Result<()> {
        self.accept_until(|| false)
    }

    /// Like `accept`, but permits an idle service to stop without closing another thread's handle.
    pub fn accept_until(&mut self, cancelled: impl Fn() -> bool) -> io::Result<()> {
        let handle = self.file.as_raw_handle();
        loop {
            if cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Adapter stopped.",
                ));
            }
            // SAFETY: file keeps handle alive; PIPE_NOWAIT permits a null OVERLAPPED.
            if unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) } != 0 {
                break;
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|n| n as u32) {
                Some(ERROR_PIPE_CONNECTED) => break,
                Some(ERROR_PIPE_LISTENING) => thread::sleep(POLL),
                Some(ERROR_NO_DATA) => self.disconnect(),
                _ => return Err(error),
            }
        }
        self.deadline = Instant::now() + TIMEOUT;
        Ok(())
    }

    pub fn disconnect(&self) {
        // SAFETY: file owns a valid server pipe handle. Failure on an already closed client is harmless.
        unsafe {
            DisconnectNamedPipe(self.file.as_raw_handle());
        }
    }

    /// Budget left for the current complete transaction, including frame reception.
    pub fn remaining_time(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            self.check_deadline()?;
            let mut count = 0;
            // SAFETY: file owns a synchronous pipe; buf is writable for the bounded length.
            let success = unsafe {
                ReadFile(
                    self.file.as_raw_handle(),
                    buf.as_mut_ptr(),
                    buf.len().min(MAX_FRAME_SIZE) as u32,
                    &mut count,
                    std::ptr::null_mut(),
                )
            };
            if success != 0 {
                if count > 0 {
                    return Ok(count as usize);
                }
                self.wait()?;
                continue;
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|n| n as u32) {
                Some(ERROR_NO_DATA) => self.wait()?,
                Some(ERROR_BROKEN_PIPE) => return Ok(0),
                _ => return Err(error),
            }
        }
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            self.check_deadline()?;
            let mut count = 0;
            // SAFETY: file owns a synchronous pipe; buf remains readable until the call returns.
            let success = unsafe {
                WriteFile(
                    self.file.as_raw_handle(),
                    buf.as_ptr(),
                    buf.len().min(MAX_FRAME_SIZE) as u32,
                    &mut count,
                    std::ptr::null_mut(),
                )
            };
            if success == 0 {
                return Err(io::Error::last_os_error());
            }
            if count > 0 {
                return Ok(count as usize);
            }
            self.wait()?;
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn request(pid: u32, request: &Request) -> io::Result<Response> {
    let name = pipe_name(pid);
    // SAFETY: name is a valid NUL-terminated UTF-16 path. The wait is bounded.
    if unsafe { WaitNamedPipeW(name.as_ptr(), TIMEOUT.as_millis() as u32) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let path = String::from_utf16_lossy(&name[..name.len() - 1]);
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    let handle = file.as_raw_handle();
    let mut server_pid = 0;
    // SAFETY: file owns handle; server_pid is a writable u32.
    if unsafe { GetNamedPipeServerProcessId(handle, &mut server_pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if server_pid != pid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unexpected adapter process.",
        ));
    }
    let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
    // SAFETY: handle is open and mode has the correct type; optional arguments are null.
    if unsafe { SetNamedPipeHandleState(handle, &mode, std::ptr::null(), std::ptr::null()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut pipe = Pipe {
        file,
        deadline: Instant::now() + TIMEOUT,
    };
    write_frame(&mut pipe, request)?;
    let response: Response = read_frame(&mut pipe)?;
    if response.version != VERSION || response.id != request.id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Adapter response version or request ID does not match.",
        ));
    }
    // Let the server know that it can disconnect without discarding unread response bytes.
    pipe.write_all(&[1])?;
    Ok(response)
}
