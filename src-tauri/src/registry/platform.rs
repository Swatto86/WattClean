//! Which machine the app acts on: Windows itself, or (in the WebDriver
//! suite's `fake-system` build) a seeded fake under the data folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use wrc_core::System;

use crate::error::{CommandError, CommandResult};

#[cfg(feature = "fake-system")]
pub fn machine(data_dir: &Path) -> std::io::Result<(Arc<dyn System>, bool)> {
    let fake = wrc_core::fake::FakeSystem::persistent(&data_dir.join("machine"))?;
    if std::env::var_os("WATTCLEAN_TEST_NOT_ELEVATED").is_some() {
        fake.set_elevated(false);
    }
    Ok((Arc::new(fake), true))
}

#[cfg(all(windows, not(feature = "fake-system")))]
pub fn machine(_data_dir: &Path) -> std::io::Result<(Arc<dyn System>, bool)> {
    Ok((Arc::new(wrc_windows::WindowsSystem::new()), false))
}

/// Let a second launch with ordinary rights reach this copy when it runs
/// as administrator (started elevated at sign-in): see
/// `wrc_windows::accept_launch_messages`.
#[cfg(windows)]
pub fn accept_second_launch(identifier: &str) -> std::io::Result<()> {
    wrc_windows::accept_launch_messages(&format!("{identifier}-sic"), &format!("{identifier}-siw"))
}

/// Starts `exe` with administrator rights, telling it to wait until this
/// process has ended (see `shell::restarted_from`).
#[cfg(windows)]
pub fn relaunch_elevated(exe: &Path) -> CommandResult<()> {
    let args = format!("{} {}", crate::shell::AFTER_FLAG, std::process::id());
    wrc_windows::relaunch_elevated(exe, &args).map_err(|error| {
        if error.raw_os_error() == Some(1223) {
            CommandError::from("Administrator rights were not granted.")
        } else {
            CommandError::from(format!("Could not restart as administrator: {error}"))
        }
    })
}

/// Waits (at most half a minute) for the copy that restarted this one to
/// end.
#[cfg(windows)]
pub fn wait_for_exit(pid: u32) {
    if !wrc_windows::wait_for_exit(pid, std::time::Duration::from_secs(30)) {
        eprintln!("The copy of WattClean that restarted this one is still running.");
    }
}

#[cfg(not(windows))]
pub fn wait_for_exit(_pid: u32) {}

/// Where to save an export. The test machine saves into its data folder,
/// so the WebDriver suite needs no dialog.
#[cfg(feature = "fake-system")]
pub fn save_path(
    _app: &tauri::AppHandle,
    data_dir: &Path,
    name: &str,
    _filters: &[(&str, &[&str])],
) -> Option<PathBuf> {
    let folder = data_dir.join("exports");
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder.join(name))
}

/// The test machine's file to open and folder to pick: in its data folder.
#[cfg(feature = "fake-system")]
pub fn open_path(
    _app: &tauri::AppHandle,
    data_dir: &Path,
    name: &str,
    _filter: (&str, &[&str]),
) -> Option<PathBuf> {
    Some(data_dir.join("exports").join(name)).filter(|path| path.is_file())
}

#[cfg(feature = "fake-system")]
pub fn pick_folder(_app: &tauri::AppHandle, data_dir: &Path) -> Option<PathBuf> {
    let folder = data_dir.join("moved-backups");
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder)
}

/// Windows' own Open dialog.
#[cfg(not(feature = "fake-system"))]
pub fn open_path(
    app: &tauri::AppHandle,
    _data_dir: &Path,
    _name: &str,
    filter: (&str, &[&str]),
) -> Option<PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .set_title("Open")
        .add_filter(filter.0, filter.1)
        .blocking_pick_file()
        .and_then(|file| file.into_path().ok())
}

/// Windows' own folder picker.
#[cfg(not(feature = "fake-system"))]
pub fn pick_folder(app: &tauri::AppHandle, _data_dir: &Path) -> Option<PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .set_title("Choose the folder for backups")
        .blocking_pick_folder()
        .and_then(|folder| folder.into_path().ok())
}

/// Windows' own Save dialog.
#[cfg(not(feature = "fake-system"))]
pub fn save_path(
    app: &tauri::AppHandle,
    _data_dir: &Path,
    name: &str,
    filters: &[(&str, &[&str])],
) -> Option<PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    let mut dialog = app.dialog().file().set_title("Save as").set_file_name(name);
    for (label, extensions) in filters {
        dialog = dialog.add_filter(*label, extensions);
    }
    dialog
        .blocking_save_file()
        .and_then(|file| file.into_path().ok())
}
