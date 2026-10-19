//! Thin, safe wrappers over the Win32 calls this crate makes.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GetDiskFreeSpaceExW, GetFileInformationByHandle, GetLongPathNameW, MOVEFILE_DELAY_UNTIL_REBOOT,
    MoveFileExW,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, CountClipboardFormats, EmptyClipboard, OpenClipboard,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{
    SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI,
    SHERB_NOSOUND, SHEmptyRecycleBinW, SHGetKnownFolderPath, SHQUERYRBINFO, SHQueryRecycleBinW,
    ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_sys::core::GUID;

/// A NUL-terminated UTF-16 copy of `text`.
pub fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    text.as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn hresult(code: i32) -> io::Result<()> {
    if code >= 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

/// A handle closed when dropped.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful Win32 call and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

pub fn known_folder(id: &GUID) -> Option<String> {
    let mut path = null_mut();
    // SAFETY: `id` is a valid GUID; on success Windows allocates `path`,
    // which is freed below with CoTaskMemFree whatever the result.
    let result = unsafe { SHGetKnownFolderPath(id, 0, null_mut(), &mut path) };
    let text = if result >= 0 && !path.is_null() {
        // SAFETY: on success `path` is a NUL-terminated wide string.
        let len = (0..).take_while(|&i| unsafe { *path.add(i) } != 0).count();
        // SAFETY: `len` characters were just read from the same buffer.
        let slice = unsafe { std::slice::from_raw_parts(path, len) };
        Some(OsString::from_wide(slice).to_string_lossy().to_string())
    } else {
        None
    };
    // SAFETY: freeing null is allowed; otherwise the buffer is Windows' own.
    unsafe { CoTaskMemFree(path.cast()) };
    text.filter(|t| !t.is_empty())
}

/// `C:\Users\SWATTO~1` as `C:\Users\Swatto`, so paths compare equal.
pub fn long_path(path: &str) -> String {
    let short = wide(path);
    let mut buffer = vec![0u16; 1024];
    // SAFETY: both buffers are valid; the length passed is the buffer's own.
    let len = unsafe { GetLongPathNameW(short.as_ptr(), buffer.as_mut_ptr(), 1024) };
    match usize::try_from(len) {
        Ok(n) if n > 0 && n < buffer.len() => OsString::from_wide(&buffer[..n])
            .to_string_lossy()
            .to_string(),
        _ => path.to_string(),
    }
}

pub fn recycle_bin() -> io::Result<(u64, u64)> {
    let mut info = SHQUERYRBINFO {
        cbSize: u32::try_from(std::mem::size_of::<SHQUERYRBINFO>()).unwrap_or(24),
        i64Size: 0,
        i64NumItems: 0,
    };
    // SAFETY: a null root means every drive; `info` is sized as Windows expects.
    hresult(unsafe { SHQueryRecycleBinW(null(), &mut info) })?;
    Ok((
        u64::try_from(info.i64NumItems).unwrap_or(0),
        u64::try_from(info.i64Size).unwrap_or(0),
    ))
}

pub fn empty_recycle_bin() -> io::Result<()> {
    // SAFETY: no window, every drive, no confirmation, progress or sound.
    let result = unsafe {
        SHEmptyRecycleBinW(
            null_mut(),
            null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    };
    // E_UNEXPECTED (0x8000FFFF) is what an already-empty bin answers.
    if result == -2_147_418_113 {
        Ok(())
    } else {
        hresult(result)
    }
}

/// Runs `work` with the clipboard open, closing it afterwards.
fn with_clipboard<T>(work: impl FnOnce() -> T) -> io::Result<T> {
    // SAFETY: no owner window; the clipboard is closed below when opened.
    if unsafe { OpenClipboard(null_mut()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = work();
    // SAFETY: this thread opened the clipboard just above.
    unsafe { CloseClipboard() };
    Ok(result)
}

pub fn clipboard_has_data() -> bool {
    // SAFETY: CountClipboardFormats only reads the clipboard's state.
    unsafe { CountClipboardFormats() > 0 }
}

pub fn clear_clipboard() -> io::Result<()> {
    // SAFETY: called with the clipboard open, as EmptyClipboard requires.
    let emptied = with_clipboard(|| unsafe { EmptyClipboard() })?;
    if emptied == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Executable names (lower case) of every running process.
pub fn running_processes() -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    // SAFETY: a process snapshot; the handle is owned and closed by `Owned`.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
        return names;
    }
    let snapshot = Owned(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0),
        ..PROCESSENTRY32W::default()
    };
    // SAFETY: `entry` is sized as the API requires and lives across the loop.
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while more {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        names.insert(String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase());
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    names
}

pub fn is_elevated() -> bool {
    let mut token = null_mut();
    // SAFETY: the pseudo-handle needs no closing; the token is owned below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let token = Owned(token);
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0u32;
    let size = u32::try_from(std::mem::size_of::<TOKEN_ELEVATION>()).unwrap_or(4);
    // SAFETY: the buffer is a TOKEN_ELEVATION of the size passed.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&raw mut elevation).cast(),
            size,
            &mut returned,
        )
    };
    ok != 0 && elevation.TokenIsElevated != 0
}

pub fn free_space(path: &Path) -> io::Result<u64> {
    let mut free = 0u64;
    let path = wide(path);
    // SAFETY: the out-pointers are valid or null, as the API allows.
    if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, null_mut(), null_mut()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(free)
}

pub fn total_space(root: &str) -> Option<(u64, u64)> {
    let (mut free, mut total) = (0u64, 0u64);
    let path = wide(root);
    // SAFETY: as in `free_space`.
    let ok = unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, &mut total, null_mut()) };
    (ok != 0).then_some((free, total))
}

pub fn delete_on_reboot(path: &Path) -> io::Result<()> {
    let path = wide(path);
    // SAFETY: a null new name means "delete"; the flag queues it for restart.
    if unsafe { MoveFileExW(path.as_ptr(), null(), MOVEFILE_DELAY_UNTIL_REBOOT) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// How many names (hard links) a file's data has. Opened for its
/// attributes only, sharing everything, without following a link.
pub fn link_count(path: &Path) -> io::Result<u32> {
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle stays open for the call and `info` is writable.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info.nNumberOfLinks)
}

pub fn shell_changed() {
    let event = i32::try_from(SHCNE_ASSOCCHANGED).unwrap_or(0x0800_0000);
    // SAFETY: an association-changed notice carries no items.
    unsafe { SHChangeNotify(event, SHCNF_IDLIST, null(), null()) };
}

/// Starts `exe` with `arguments` as administrator (a UAC prompt).
pub fn run_elevated(exe: &Path, arguments: &str) -> io::Result<()> {
    let verb = wide("runas");
    let file = wide(exe);
    let parameters = wide(arguments);
    // SAFETY: every string is NUL-terminated and outlives the call.
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as a value above 32.
    if result as usize > 32 {
        Ok(())
    } else {
        Err(io::Error::other(
            "Windows did not start the program as administrator (the prompt may have been declined)",
        ))
    }
}
