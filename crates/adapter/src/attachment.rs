//! Host-side validation and ownership of one thread-specific game hook.

use crate::bridge::{START, STOP, message_id};
use dd_protocol::{Command, Request, pipe};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HMODULE, HWND, LPARAM, WAIT_TIMEOUT},
    System::{
        LibraryLoader::GetProcAddress,
        SystemInformation::{IMAGE_FILE_MACHINE_I386, IMAGE_FILE_MACHINE_UNKNOWN},
        Threading::{
            CreateMutexW, IsWow64Process2, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, HHOOK, HOOKPROC,
        IsWindowVisible, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_GETMESSAGE,
    },
};

const GAME_HASH: &str = "2b43fef8ecf629d72b9a3fc032a65d6bf0298fd983301e97cdbfb45fd6314883";

struct Target {
    process: OwnedHandle,
    thread: u32,
}

struct WindowSearch {
    pid: u32,
    threads: Vec<u32>,
}

unsafe extern "system" fn find_window(window: HWND, context: LPARAM) -> i32 {
    // SAFETY: EnumWindows receives our exclusively borrowed WindowSearch as context.
    let search = unsafe { &mut *(context as *mut WindowSearch) };
    let mut pid = 0;
    // SAFETY: window is provided by EnumWindows and pid is writable.
    let thread = unsafe { GetWindowThreadProcessId(window, &mut pid) };
    // SAFETY: these APIs accept enumerated HWNDs and retain no pointers.
    let visible_unowned =
        unsafe { IsWindowVisible(window) != 0 && GetWindow(window, GW_OWNER).is_null() };
    if pid == search.pid && thread != 0 && visible_unowned {
        search.threads.push(thread);
    }
    1
}

fn verified_target(pid: u32) -> io::Result<Target> {
    if usize::BITS != 32 || pid == 0 || pid == std::process::id() {
        return Err(io::Error::other(
            "Attachment requires the 32-bit host and a separate game process.",
        ));
    }
    // SAFETY: only query/synchronize access is requested; returned handle is checked and owned below.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcess returned a uniquely owned non-null handle.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut machine = 0;
    let mut native = 0;
    // SAFETY: process is live and both output variables have the required type.
    if unsafe { IsWow64Process2(process.as_raw_handle(), &mut machine, &mut native) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if machine != IMAGE_FILE_MACHINE_I386
        && !(machine == IMAGE_FILE_MACHINE_UNKNOWN && native == IMAGE_FILE_MACHINE_I386)
    {
        return Err(io::Error::other("The target must be a 32-bit x86 process."));
    }
    let mut path = vec![0; 32768];
    let mut length = path.len() as u32;
    // SAFETY: output buffer is writable for length UTF-16 units and process has query rights.
    if unsafe {
        QueryFullProcessImageNameW(process.as_raw_handle(), 0, path.as_mut_ptr(), &mut length)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(&path[..length as usize]));
    if !path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("DunDefGame.exe"))
    {
        return Err(io::Error::other("The target is not DunDefGame.exe."));
    }
    let mut file = File::open(path)?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)?;
    if &dos[..2] != b"MZ" {
        return Err(io::Error::other("Invalid game executable."));
    }
    let pe = u32::from_le_bytes(dos[60..64].try_into().expect("four bytes"));
    file.seek(SeekFrom::Start(pe.into()))?;
    let mut signature = [0; 6];
    file.read_exact(&mut signature)?;
    if &signature != b"PE\0\0\x4c\x01" {
        return Err(io::Error::other("Game executable is not PE32 x86."));
    }
    file.rewind()?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    if format!("{:x}", digest.finalize()) != GAME_HASH {
        return Err(io::Error::other(
            "This game build has not been verified. Attachment refused.",
        ));
    }
    let mut search = WindowSearch {
        pid,
        threads: Vec::new(),
    };
    // SAFETY: callback/context have the matching signatures and remain valid for enumeration.
    if unsafe { EnumWindows(Some(find_window), &mut search as *mut WindowSearch as isize) } == 0 {
        return Err(io::Error::last_os_error());
    }
    search.threads.sort_unstable();
    search.threads.dedup();
    if search.threads.len() != 1 {
        return Err(io::Error::other(
            "Could not identify one visible game window thread.",
        ));
    }
    Ok(Target {
        process,
        thread: search.threads[0],
    })
}

struct Hook(HHOOK);

fn claim_attachment(pid: u32) -> io::Result<OwnedHandle> {
    let name: Vec<u16> = format!("Local\\DungeonWorkshop.Attach.{pid}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: name is NUL terminated; default security applies; no initial ownership is requested.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    // SAFETY: capture this thread's last error immediately after CreateMutexW.
    let error = unsafe { GetLastError() };
    if handle.is_null() {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    // SAFETY: CreateMutexW transferred a uniquely owned handle. Object lifetime provides the lease.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    if error == ERROR_ALREADY_EXISTS {
        return Err(io::Error::other(
            "An overlay is already attached or connecting to this game.",
        ));
    }
    Ok(handle)
}

impl Drop for Hook {
    fn drop(&mut self) {
        // SAFETY: this object uniquely owns the hook. Target DLL pins itself before starting work.
        unsafe {
            UnhookWindowsHookEx(self.0);
        }
    }
}

/// Attach only to the verified game's window thread. Keep stdin piped open while connected.
///
/// # Safety
/// `module` must be the loaded adjacent adapter DLL and must remain loaded until this returns.
pub unsafe fn run(pid: u32, module: HMODULE) -> Result<(), Box<dyn std::error::Error>> {
    let target = verified_target(pid)?;
    let _lease = claim_attachment(pid)?;
    let message = message_id();
    if message == 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: caller guarantees module is live; symbol is NUL terminated.
    let address = unsafe { GetProcAddress(module, c"dd_adapter_hook".as_ptr().cast()) }
        .ok_or("Adapter does not export dd_adapter_hook.")?;
    // SAFETY: this exact hook ABI is exported by the adjacent adapter built with the host.
    let callback: HOOKPROC = Some(unsafe {
        std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn(i32, usize, isize) -> isize,
        >(address)
    });
    // SAFETY: thread is nonzero and belongs to the hash/architecture-verified game; DLL ABI matches.
    let hook = unsafe { SetWindowsHookExW(WH_GETMESSAGE, callback, module, target.thread) };
    if hook.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    let _hook = Hook(hook);
    let owner = std::process::id() as usize;
    // SAFETY: thread queue belongs to the verified game window; message contains integers only.
    if unsafe { PostThreadMessageW(target.thread, message, owner, START) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let closed = Arc::new(AtomicBool::new(false));
    let input_closed = Arc::clone(&closed);
    std::thread::spawn(move || {
        let _ = io::stdin().read(&mut [0]);
        input_closed.store(true, Ordering::Release);
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut ready = false;
    while Instant::now() < deadline && !closed.load(Ordering::Acquire) {
        // SAFETY: target process handle remains owned throughout attachment.
        if unsafe { WaitForSingleObject(target.process.as_raw_handle(), 0) } != WAIT_TIMEOUT {
            break;
        }
        if let Ok(response) = pipe::request(pid, &Request::new(0, Command::Status {})) {
            match response.result {
                Ok(status) if status.pid == pid && status.bits == 32 && !status.diagnostic => {
                    ready = true;
                    break;
                }
                Ok(_) => break,
                Err(rejection) => return Err(rejection.message.into()),
            }
        }
        // A prior host's idle worker may need one polling interval to observe its owner's exit.
        // SAFETY: repeat an idempotent bootstrap only on the verified nonzero game thread.
        unsafe {
            PostThreadMessageW(target.thread, message, owner, START);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if ready {
        println!("Attached to game process {pid}.");
        while !closed.load(Ordering::Acquire) {
            // SAFETY: target process handle remains owned; waiting is bounded.
            if unsafe { WaitForSingleObject(target.process.as_raw_handle(), 100) } != WAIT_TIMEOUT {
                break;
            }
        }
    }
    // SAFETY: best-effort shutdown addressed to the same nonzero thread; no pointers cross processes.
    unsafe {
        PostThreadMessageW(target.thread, message, owner, STOP);
    }
    // The target DLL is pinned; an in-flight callback/worker cannot execute from unmapped memory.
    // Idle IPC observes the owner's process exit even if the game is no longer pumping messages.
    if !ready {
        return Err("Game adapter did not become ready within 15 seconds.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_attachment_per_process_until_owner_releases_handle() {
        let pid = std::process::id();
        let first = claim_attachment(pid).unwrap();
        assert!(claim_attachment(pid).is_err());
        drop(first);
        assert!(claim_attachment(pid).is_ok());
    }
}
