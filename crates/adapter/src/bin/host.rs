//! Loads the adapter for isolated diagnostics or attaches it to one verified game thread.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::FreeLibrary,
        System::LibraryLoader::{
            GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
            LoadLibraryExW,
        },
    };
    let args: Vec<_> = std::env::args().skip(1).collect();
    let target = match args.as_slice() {
        [] => None,
        [flag, pid] if flag == "--attach" => Some(pid.parse::<u32>()?),
        _ => return Err("Usage: dd-adapter-host [--attach GAME_PID]".into()),
    };
    let dll = std::env::current_exe()?
        .with_file_name("dd_adapter.dll")
        .canonicalize()?;
    let path: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: absolute NUL-terminated path; dependencies restricted to DLL and system directories.
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = if let Some(pid) = target {
        // SAFETY: module is the adjacent adapter and stays loaded for all of attachment::run.
        unsafe { dd_adapter::attachment::run(pid, module) }
    } else {
        // SAFETY: module is loaded and symbol is NUL terminated.
        match unsafe { GetProcAddress(module, c"dd_adapter_run".as_ptr().cast()) } {
            Some(address) => {
                // SAFETY: this exact ABI is exported by the adapter built alongside this host.
                let run: extern "system" fn() -> u32 = unsafe { std::mem::transmute(address) };
                let code = run();
                if code == 0 {
                    Ok(())
                } else {
                    Err(format!("Adapter exited with code {code}.").into())
                }
            }
            None => Err("Adapter does not export dd_adapter_run.".into()),
        }
    };
    // SAFETY: diagnostics have returned, or the host's hook was removed before returning.
    unsafe {
        FreeLibrary(module);
    }
    result
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The adapter host requires Windows.");
}
