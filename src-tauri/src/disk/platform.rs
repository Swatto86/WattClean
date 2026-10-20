//! Which machine the app acts on (Windows, Linux or macOS itself, or the
//! sandboxed test machine in the WebDriver suite's `fake-system` build),
//! and the native pieces the commands use: Open dialogs, the file manager,
//! administrator restart, macOS's Full Disk Access.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use wcc_core::system::System;

use crate::error::{CommandError, CommandResult};

#[cfg(feature = "fake-system")]
pub fn machine(data_dir: &Path) -> std::io::Result<(Arc<dyn System>, bool)> {
    Ok((Arc::new(wcc_core::fake::FakeSystem::open(data_dir)?), true))
}

#[cfg(all(windows, not(feature = "fake-system")))]
pub fn machine(_data_dir: &Path) -> std::io::Result<(Arc<dyn System>, bool)> {
    Ok((Arc::new(wcc_windows::WindowsSystem::new()), false))
}

#[cfg(all(unix, not(feature = "fake-system")))]
pub fn machine(_data_dir: &Path) -> std::io::Result<(Arc<dyn System>, bool)> {
    Ok((Arc::new(wcc_unix::UnixSystem::new()), false))
}

/// What an Open dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Files,
    Folder,
    Ini,
}

/// The test machine's dialogs answer from `picks.json` in its data folder,
/// which the WebDriver suite writes before it clicks; no file means
/// "cancelled".
#[cfg(feature = "fake-system")]
pub fn pick(_app: &tauri::AppHandle, data_dir: &Path, what: Pick, _title: &str) -> Vec<PathBuf> {
    let picks: serde_json::Value =
        wcc_core::store::load(&data_dir.join("picks.json")).unwrap_or_default();
    let field = match what {
        Pick::Files => "files",
        Pick::Folder => "folder",
        Pick::Ini => "ini",
    };
    match &picks[field] {
        serde_json::Value::String(one) => vec![PathBuf::from(one)],
        serde_json::Value::Array(many) => many
            .iter()
            .filter_map(|v| v.as_str())
            .map(PathBuf::from)
            .collect(),
        _ => Vec::new(),
    }
}

/// Windows' own Open dialog.
#[cfg(not(feature = "fake-system"))]
pub fn pick(app: &tauri::AppHandle, _data_dir: &Path, what: Pick, title: &str) -> Vec<PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file().set_title(title);
    let to_paths = |files: Vec<tauri_plugin_dialog::FilePath>| -> Vec<PathBuf> {
        files
            .into_iter()
            .filter_map(|f| f.into_path().ok())
            .collect()
    };
    match what {
        Pick::Files => dialog
            .blocking_pick_files()
            .map(to_paths)
            .unwrap_or_default(),
        Pick::Folder => dialog
            .blocking_pick_folder()
            .map(|f| to_paths(vec![f]))
            .unwrap_or_default(),
        Pick::Ini => dialog
            .add_filter("winapp2.ini", &["ini"])
            .blocking_pick_file()
            .map(|f| to_paths(vec![f]))
            .unwrap_or_default(),
    }
}

/// Opens a folder of the app's own in the file manager (Explorer, Finder,
/// or the desktop's own through `xdg-open`, which may wait for the file
/// manager to close: it is reaped when it ends).
pub fn open_folder(fake: bool, folder: &Path) -> CommandResult<()> {
    std::fs::create_dir_all(folder)?;
    if fake {
        return Ok(());
    }
    #[cfg(windows)]
    let mut opener = std::process::Command::new("explorer.exe");
    // The system's own environment, not an AppImage's.
    #[cfg(target_os = "macos")]
    let mut opener = wcc_unix::process::command("/usr/bin/open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut opener = wcc_unix::process::command("xdg-open");
    let mut child = opener.arg(folder).spawn().map_err(|error| {
        CommandError::from(format!("The file manager could not be opened: {error}"))
    })?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The community's winapp2.ini, as BleachBit downloads it.
pub const WINAPP2_URL: &str =
    "https://raw.githubusercontent.com/MoscaDotTo/Winapp2/master/Winapp2.ini";

/// winapp2.ini describes Windows programs only.
#[cfg(not(windows))]
pub fn download(_url: &str, _to: &Path) -> CommandResult<()> {
    Err("winapp2.ini describes Windows programs: it is offered on Windows only.".into())
}

/// Downloads a file with Windows' own curl.exe (Windows 10 1803 and later):
/// arguments are passed one by one, never through a shell.
#[cfg(windows)]
pub fn download(url: &str, to: &Path) -> CommandResult<()> {
    let curl = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join(r"System32\curl.exe"))
        .filter(|path| path.is_file())
        .ok_or("curl.exe was not found in Windows' System32 folder.")?;
    let output = std::process::Command::new(curl)
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--max-time",
            "120",
            "--output",
        ])
        .arg(to)
        .arg(url)
        .output()
        .map_err(|error| CommandError::from(format!("curl could not start: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "The download failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into())
    }
}

/// System Settings › Privacy & Security › Full Disk Access.
#[cfg(target_os = "macos")]
const FULL_DISK_ACCESS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// Whether macOS lets WattClean read what Full Disk Access protects:
/// Safari's folder (nearly every account has one), else the system's privacy
/// database folder. `None` when neither could tell.
#[cfg(target_os = "macos")]
pub fn full_disk_access() -> Option<bool> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    access_by_listing(&[
        home.join("Library/Safari"),
        PathBuf::from("/Library/Application Support/com.apple.TCC"),
    ])
}

/// Full Disk Access is macOS's.
#[cfg(not(target_os = "macos"))]
pub fn full_disk_access() -> Option<bool> {
    None
}

/// Opens System Settings at Full Disk Access, by a fixed address: macOS
/// asks to quit and reopen WattClean once access is given.
#[cfg(target_os = "macos")]
pub fn open_privacy_settings() -> CommandResult<()> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg(FULL_DISK_ACCESS)
        .status()
        .map_err(|error| {
            CommandError::from(format!("System Settings could not be opened: {error}"))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("System Settings could not be opened ({status}).").into())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn open_privacy_settings() -> CommandResult<()> {
    Err("Full Disk Access is a macOS setting.".into())
}

/// Whether `protected` can be read, from listing the first one that is
/// there.
#[cfg(any(target_os = "macos", test))]
fn access_by_listing(protected: &[PathBuf]) -> Option<bool> {
    protected
        .iter()
        .find_map(|folder| verdict(std::fs::read_dir(folder).map(|_| ())))
}

/// What listing one protected folder says: listed, access; refused by
/// macOS's privacy protection (EPERM, not a file's permissions), none;
/// anything else (it is not there), nothing.
#[cfg(any(target_os = "macos", test))]
fn verdict(listed: std::io::Result<()>) -> Option<bool> {
    match listed {
        Ok(()) => Some(true),
        Err(error) if error.raw_os_error() == Some(1) => Some(false),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_macos_privacy_refusal_means_no_full_disk_access() {
        let refused = std::io::Error::from_raw_os_error(1); // EPERM
        assert_eq!(verdict(Err(refused)), Some(false));
        assert_eq!(verdict(Ok(())), Some(true));
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(verdict(Err(missing)), None);
        let denied = std::io::Error::from_raw_os_error(13); // EACCES
        assert_eq!(verdict(Err(denied)), None, "file permissions are not TCC");
    }

    #[test]
    fn the_first_protected_folder_that_is_there_decides() {
        let home = tempfile::tempdir().expect("temp folder");
        let safari = home.path().join("Library/Safari");
        let elsewhere = home.path().join("Library/Elsewhere");
        assert_eq!(
            access_by_listing(&[safari.clone(), elsewhere.clone()]),
            None
        );
        std::fs::create_dir_all(&elsewhere).expect("folder");
        assert_eq!(access_by_listing(&[safari, elsewhere]), Some(true));
    }
}
