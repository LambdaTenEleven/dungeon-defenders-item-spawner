use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub const ANALYZED_SHA256: &str =
    "4e4b03765c9e74415e27e36d952ebecd86a655ca27d8a46b5285279773e6d0f4";

pub fn find_installation() -> Option<PathBuf> {
    let mut roots = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles(x86)") {
        roots.push(PathBuf::from(program_files).join("Steam"));
    }
    for drive in b'C'..=b'Z' {
        roots.push(PathBuf::from(format!("{}:/SteamLibrary", drive as char)));
    }
    // Read Steam's library list; no registry edits or recursive disk scan.
    for root in roots.clone() {
        if let Ok(libraries) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            for line in libraries.lines() {
                let fields: Vec<_> = line.split('"').collect();
                if fields.get(1) == Some(&"path")
                    && let Some(path) = fields.get(3)
                {
                    roots.push(PathBuf::from(path.replace("\\\\", "\\")));
                }
            }
        }
    }
    roots
        .into_iter()
        .map(|root| root.join("steamapps/common/Dungeon Defenders"))
        .find(|root| root.join("Binaries/Win32/DunDefGame.exe").is_file())
}

pub fn inspect(root: &Path) -> io::Result<String> {
    let path = root.join("Binaries/Win32/DunDefGame.exe");
    let mut file = File::open(path)?;
    let mut header = [0; 64];
    file.read_exact(&mut header)?;
    if &header[..2] != b"MZ" {
        return Err(io::Error::other("Not a Windows executable."));
    }
    let pe = u32::from_le_bytes(header[60..64].try_into().expect("four bytes"));
    file.seek(SeekFrom::Start(pe.into()))?;
    let mut signature = [0; 6];
    file.read_exact(&mut signature)?;
    if &signature[..4] != b"PE\0\0" || u16::from_le_bytes([signature[4], signature[5]]) != 0x14c {
        return Err(io::Error::other(
            "Expected the 32-bit Dungeon Defenders executable.",
        ));
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
    let hash = format!("{:x}", digest.finalize());
    if hash == ANALYZED_SHA256 {
        Ok("Recognized 32-bit build. Start local single-player, then connect to the game.".into())
    } else {
        Ok(format!(
            "Unrecognized build ({hash}). Item requests are disabled."
        ))
    }
}

#[cfg(windows)]
pub fn process_ids() -> Vec<u32> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    // SAFETY: TH32CS_SNAPPROCESS requires no caller pointers; returned handle is checked.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    // SAFETY: PROCESSENTRY32W is plain data with no invalid all-zero bit patterns.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: snapshot is valid and entry is initialized with its documented size.
    let mut found = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    let mut processes = Vec::new();
    while found {
        let end = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..end]).eq_ignore_ascii_case("DunDefGame.exe")
        {
            processes.push(entry.th32ProcessID);
        }
        // SAFETY: snapshot and entry remain valid throughout enumeration.
        found = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: snapshot is uniquely owned and no longer used.
    unsafe {
        CloseHandle(snapshot);
    }
    processes
}

#[cfg(not(windows))]
pub fn process_ids() -> Vec<u32> {
    Vec::new()
}

pub fn running() -> bool {
    !process_ids().is_empty()
}
