//! Loads the real adapter DLL into an isolated process for integration tests.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW,
    };
    let dll = std::env::current_exe()?
        .with_file_name("dd_adapter.dll")
        .canonicalize()?;
    let path: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: absolute, NUL-terminated path; dependency lookup is limited to DLL and system directories.
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
    // SAFETY: module is loaded and the symbol is NUL terminated.
    let address = unsafe { GetProcAddress(module, c"dd_adapter_run".as_ptr().cast()) };
    let Some(address) = address else {
        // SAFETY: module is still owned here and no function from it has been called.
        unsafe {
            FreeLibrary(module);
        }
        return Err("Adapter does not export dd_adapter_run.".into());
    };
    // SAFETY: this is our exported function with this exact ABI and signature, built alongside the host.
    let run: extern "system" fn() -> u32 = unsafe { std::mem::transmute(address) };
    let result = run();
    // SAFETY: run has returned; the adapter creates no background threads or retained callbacks.
    unsafe {
        FreeLibrary(module);
    }
    if result != 0 {
        return Err(format!("Adapter exited with code {result}.").into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The adapter host requires Windows.");
}
