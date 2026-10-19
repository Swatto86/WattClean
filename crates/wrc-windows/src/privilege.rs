//! Administrator rights: whether this process has them, and restarting
//! with them through the normal UAC prompt.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// `ERROR_ELEVATION_REQUIRED`: Windows refused to start a program that
/// demands administrator rights from a process that lacks them.
pub const ELEVATION_REQUIRED: i32 = 740;

pub fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn is_elevated() -> bool {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing;
    // `token` is a valid out-pointer for the opened token handle.
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if opened == 0 {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0u32;
    let size = u32::try_from(std::mem::size_of::<TOKEN_ELEVATION>()).unwrap_or(4);
    // SAFETY: `token` was opened with TOKEN_QUERY above; the buffer is a
    // TOKEN_ELEVATION of exactly `size` bytes.
    let queried = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            size,
            &mut returned,
        )
    };
    // SAFETY: `token` is a handle this function opened and owns.
    unsafe { CloseHandle(token) };
    queried != 0 && elevation.TokenIsElevated != 0
}

/// Start `program` again with administrator rights (Windows shows its UAC
/// prompt). An error with `ERROR_CANCELLED` (1223) means the person said no.
pub fn relaunch_elevated(program: &Path, args: &str) -> io::Result<()> {
    let verb = wide(OsStr::new("runas"));
    let file = wide(program.as_os_str());
    let parameters = wide(OsStr::new(args));
    // SAFETY: SHELLEXECUTEINFOW is a plain C struct for which all-zero bytes
    // are a valid "unset" value; the fields used are filled in below.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>()).unwrap_or(0);
    info.fMask = SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    // SAFETY: every string pointer refers to a NUL-terminated buffer that
    // outlives this call; `info` is fully initialised.
    let started = unsafe { ShellExecuteExW(&mut info) };
    if started == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The current user's SID (`S-1-5-21-…`), which names their registry hive in
/// kernel paths (`\REGISTRY\USER\<sid>`).
pub fn user_sid() -> Option<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{TOKEN_USER, TokenUser};

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: a pseudo-handle and a valid out-pointer, as in `is_elevated`.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    // TOKEN_USER plus the SID it points into; 256 bytes holds any SID.
    let mut buffer = vec![0u64; 32];
    let size = u32::try_from(buffer.len() * 8).ok()?;
    let mut returned = 0u32;
    // SAFETY: the token was opened with TOKEN_QUERY; the buffer is aligned and
    // `size` bytes long.
    let queried = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut returned,
        )
    };
    // SAFETY: `token` is owned here and closed once.
    unsafe { CloseHandle(token) };
    if queried == 0 {
        return None;
    }
    // SAFETY: GetTokenInformation filled the buffer with a TOKEN_USER.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: the SID points into `buffer`, still alive; Windows allocates `text`.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 || text.is_null() {
        return None;
    }
    // SAFETY: `text` is a NUL-terminated string Windows allocated.
    let length = (0..)
        .take_while(|&at| unsafe { *text.add(at) } != 0)
        .count();
    // SAFETY: `length` units were just read from it.
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
    // SAFETY: allocated by ConvertSidToStringSidW, released once with LocalFree.
    unsafe { LocalFree(text.cast()) };
    Some(sid)
}

/// Switch on one of this process's privileges (an administrator holds
/// SeBackupPrivilege and SeRestorePrivilege disabled until asked).
pub fn enable_privilege(name: &str) -> io::Result<()> {
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    };

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: a pseudo-handle and a valid out-pointer, as in `is_elevated`.
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: windows_sys::Win32::Foundation::LUID {
                LowPart: 0,
                HighPart: 0,
            },
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    let wide_name = wide(OsStr::new(name));
    // SAFETY: the name is NUL-terminated; the LUID out-pointer is valid.
    let looked_up = unsafe {
        LookupPrivilegeValueW(
            std::ptr::null(),
            wide_name.as_ptr(),
            &mut privileges.Privileges[0].Luid,
        )
    };
    let result = if looked_up == 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: the token was opened for adjusting; the structure holds one entry.
        let adjusted = unsafe {
            AdjustTokenPrivileges(
                token,
                0,
                &privileges,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        // ERROR_NOT_ALL_ASSIGNED (1300): the process does not hold the privilege.
        let error = io::Error::last_os_error();
        if adjusted == 0 || error.raw_os_error() == Some(1300) {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{name} is not held: {error}"),
            ))
        } else {
            Ok(())
        }
    };
    // SAFETY: `token` is owned here and closed once.
    unsafe { CloseHandle(token) };
    result
}

/// Waits until process `pid` has ended, at most `timeout`. True when it has
/// ended or no such process runs; false when it is still running (or
/// cannot be waited on).
pub fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    // SAFETY: plain values in; the handle returned is closed below.
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        // SAFETY: reads this thread's last error, set by OpenProcess.
        return unsafe { GetLastError() } == ERROR_INVALID_PARAMETER;
    }
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
    // SAFETY: `process` is a valid handle opened for waiting.
    let ended = unsafe { WaitForSingleObject(process, millis) } == WAIT_OBJECT_0;
    // SAFETY: `process` is owned here and closed once.
    unsafe { CloseHandle(process) };
    ended
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Instant;

    fn pinging(count: u32) -> std::process::Child {
        Command::new("ping")
            .args(["-n", &count.to_string(), "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    #[test]
    fn waiting_for_a_process_returns_once_it_has_ended() {
        let mut short = pinging(2);
        let started = Instant::now();
        assert!(wait_for_exit(short.id(), Duration::from_secs(20)));
        assert!(started.elapsed() < Duration::from_secs(15));
        assert!(short.try_wait().unwrap().is_some());

        let mut long = pinging(30);
        assert!(!wait_for_exit(long.id(), Duration::from_millis(200)));
        long.kill().unwrap();
        long.wait().unwrap();
        assert!(wait_for_exit(long.id(), Duration::from_secs(1)));
    }
}
