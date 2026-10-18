//! Which machine the app acts on (Windows itself, or the sandboxed test
//! machine in the WebDriver suite's `fake-system` build), and the native
//! pieces the commands use: Open dialogs, Explorer, administrator restart.

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

/// Opens a folder of the app's own in Explorer.
pub fn open_folder(fake: bool, folder: &Path) -> CommandResult<()> {
    std::fs::create_dir_all(folder)?;
    if fake {
        return Ok(());
    }
    std::process::Command::new("explorer.exe")
        .arg(folder)
        .spawn()
        .map(|_| ())
        .map_err(|error| CommandError::from(format!("Explorer could not be opened: {error}")))
}

/// The community's winapp2.ini, as BleachBit downloads it.
pub const WINAPP2_URL: &str =
    "https://raw.githubusercontent.com/MoscaDotTo/Winapp2/master/Winapp2.ini";

/// Downloads a file with Windows' own curl.exe (Windows 10 1803 and later):
/// arguments are passed one by one, never through a shell.
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
