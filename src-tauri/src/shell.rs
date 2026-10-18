//! What the whole app shares: its data folder, the shell's settings, how it
//! was started, and the commands no single module owns (about, the look,
//! administrator restart, quitting).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::settings::{ShellPatch, ShellSettings};
use crate::{disk, registry};

pub struct Shell {
    /// `%LOCALAPPDATA%\WattClean`, or `WATTCLEAN_DATA_DIR`.
    pub root: PathBuf,
    /// Started at sign-in into the notification area, with no window.
    pub started_hidden: bool,
    /// True in the WebDriver suite's build, which acts on test machines.
    pub fake: bool,
    settings: Mutex<ShellSettings>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // The settings are replaced whole, so a panic elsewhere cannot leave
    // them half-written.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `WATTCLEAN_DATA_DIR` moves the app's files (the WebDriver suite isolates
/// each run this way); otherwise `%LOCALAPPDATA%\WattClean`.
pub fn root() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("WATTCLEAN_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    local_app_data().map(|local| local.join("WattClean"))
}

pub fn local_app_data() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "Windows did not report a Local AppData folder".to_owned())
}

impl Shell {
    pub fn new(root: PathBuf, started_hidden: bool, fake: bool) -> Self {
        let settings = ShellSettings::load(&root.join("settings.json"));
        Self {
            root,
            started_hidden,
            fake,
            settings: Mutex::new(settings),
        }
    }

    pub fn settings(&self) -> ShellSettings {
        lock(&self.settings).clone()
    }

    /// Closing the window leaves the app in the notification area when it
    /// is set to, or was started at sign-in; otherwise closing quits.
    pub fn keeps_running(&self) -> bool {
        self.started_hidden || self.settings().keep_in_tray
    }

    /// Saves first, then keeps: a failed save leaves the old settings.
    fn update(&self, patch: ShellPatch) -> CommandResult<ShellSettings> {
        let next = self.settings().patched(patch)?;
        next.save(&self.root.join("settings.json"))
            .map_err(|error| {
                CommandError::from(format!("The settings could not be saved: {error}"))
            })?;
        *lock(&self.settings) = next.clone();
        Ok(next)
    }
}

type ShellState<'a> = State<'a, Arc<Shell>>;
type Disk<'a> = State<'a, Arc<disk::state::AppState>>;
type Registry<'a> = State<'a, Arc<registry::state::AppState>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    elevated: bool,
    test_machine: bool,
    data_dir: String,
    cleaners_dir: String,
    windows_build: Option<u32>,
    /// Started at sign-in: the page stays hidden until opened from the tray.
    started_hidden: bool,
}

#[tauri::command]
pub fn app_info(
    app: AppHandle,
    shell: ShellState<'_>,
    disk: Disk<'_>,
    registry: Registry<'_>,
) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        // One answer for the whole app; every module's machine agrees on a
        // real PC.
        elevated: registry.system.is_elevated(),
        test_machine: shell.fake,
        data_dir: shell.root.display().to_string(),
        cleaners_dir: disk.cleaners_dir().display().to_string(),
        windows_build: wrc_core::tuner::windows_build(registry.system.as_ref()),
        started_hidden: shell.started_hidden,
    }
}

#[tauri::command]
pub fn get_shell_settings(shell: ShellState<'_>) -> ShellSettings {
    shell.settings()
}

#[tauri::command]
pub fn update_shell_settings(
    shell: ShellState<'_>,
    patch: ShellPatch,
) -> CommandResult<ShellSettings> {
    shell.update(patch)
}

/// Opens WattClean's data folder in File Explorer.
#[tauri::command]
pub fn open_data_folder(shell: ShellState<'_>) -> CommandResult<()> {
    disk::platform::open_folder(shell.fake, &shell.root)
}

/// Closes the app, as the tray's Quit does (after a clean, when "Exit
/// after cleaning" is on, or from Settings).
#[tauri::command]
pub fn quit_app(app: AppHandle, disk: Disk<'_>) -> CommandResult<()> {
    if disk.is_busy() {
        return Err("Wait for the running clean, shred or wipe to finish first.".into());
    }
    app.exit(0);
    Ok(())
}

/// Starts again with administrator rights, then closes this copy.
#[tauri::command]
pub fn restart_as_admin(
    app: AppHandle,
    shell: ShellState<'_>,
    disk: Disk<'_>,
) -> CommandResult<()> {
    if shell.fake {
        return Err("The test machine has no administrator prompt.".into());
    }
    if disk.is_busy() {
        return Err("Wait for the running clean, shred or wipe to finish first.".into());
    }
    let exe = std::env::current_exe()
        .map_err(|error| CommandError::from(format!("Could not find this program: {error}")))?;
    relaunch(&exe)?;
    app.exit(0);
    Ok(())
}

#[cfg(windows)]
fn relaunch(exe: &std::path::Path) -> CommandResult<()> {
    registry::platform::relaunch_elevated(exe)
}

#[cfg(not(windows))]
fn relaunch(_exe: &std::path::Path) -> CommandResult<()> {
    Err("Restarting as administrator needs Windows.".into())
}

/// Quits once the running disk job (a wipe gives its space back when
/// stopped) has stopped, waiting at most a minute.
pub fn quit_when_idle(app: &AppHandle) {
    let Some(disk) = app.try_state::<Arc<disk::state::AppState>>() else {
        app.exit(0);
        return;
    };
    if !disk.is_busy() {
        app.exit(0);
        return;
    }
    disk.cancel();
    let disk = Arc::clone(&disk);
    let app = app.clone();
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while disk.is_busy() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        app.exit(0);
    });
}
