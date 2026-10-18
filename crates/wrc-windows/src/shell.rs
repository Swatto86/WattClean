//! Registry Editor and File Explorer.

use std::ffi::OsStr;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use windows_sys::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use crate::privilege::wide;

/// Keeps console tools from flashing a window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn windows_dir() -> PathBuf {
    std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from)
}

/// Start a new Registry Editor at `long_name` (`HKEY_CURRENT_USER\…`).
/// Registry Editor opens at the key it last showed, so that is set first;
/// `-m` starts a new window even when one is already open.
pub fn open_in_regedit(long_name: &str) -> io::Result<()> {
    let (applet, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Applets\Regedit")?;
    applet.set_value("LastKey", &format!(r"Computer\{long_name}"))?;
    shell_execute(&windows_dir().join("regedit.exe"), "-m")
}

/// Open a web address in the default browser.
pub fn open_url(url: &str) -> io::Result<()> {
    shell_execute(Path::new(url), "")
}

/// Open `program` as the shell would; Registry Editor asks for
/// administrator rights itself.
fn shell_execute(program: &Path, args: &str) -> io::Result<()> {
    let verb = wide(OsStr::new("open"));
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
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// End File Explorer; Windows starts the shell again by itself. If it has
/// not within a few seconds, start it.
pub fn restart_explorer() -> io::Result<()> {
    let system32 = windows_dir().join("System32");
    let ended = Command::new(system32.join("taskkill.exe"))
        .args(["/F", "/IM", "explorer.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    // 128: no explorer.exe was running, so there is only starting it left.
    if !ended.status.success() && ended.status.code() != Some(128) {
        return Err(io::Error::other(format!(
            "taskkill could not end File Explorer: {}",
            String::from_utf8_lossy(&ended.stderr).trim()
        )));
    }
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(300));
        if explorer_running(&system32)? {
            return Ok(());
        }
    }
    Command::new(windows_dir().join("explorer.exe")).spawn()?;
    Ok(())
}

fn explorer_running(system32: &Path) -> io::Result<bool> {
    let output = Command::new(system32.join("tasklist.exe"))
        .args(["/FI", "IMAGENAME eq explorer.exe", "/NH", "/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .to_ascii_lowercase()
        .contains("\"explorer.exe\""))
}
