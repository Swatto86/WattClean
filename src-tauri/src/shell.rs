//! What the whole app shares: its data folder, the shell's settings, how it
//! was started, and the commands no single module owns (about, the look,
//! administrator restart, quitting).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::disk;
use crate::error::{CommandError, CommandResult};
#[cfg(windows)]
use crate::registry;
use crate::settings::{ShellPatch, ShellSettings};

pub struct Shell {
    /// `%LOCALAPPDATA%\WattClean`, or `WATTCLEAN_DATA_DIR`.
    pub root: PathBuf,
    /// Started at sign-in into the notification area, with no window.
    pub started_hidden: bool,
    /// True in the WebDriver suite's build, which acts on test machines.
    pub fake: bool,
    /// What could not be copied from the apps WattClean replaces at this
    /// start (it is tried again at the next).
    adopt_problems: Vec<String>,
    settings: Mutex<ShellSettings>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // The settings are replaced whole, so a panic elsewhere cannot leave
    // them half-written.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `WATTCLEAN_DATA_DIR` moves the app's files (the WebDriver suite isolates
/// each run this way); otherwise this platform's data folder
/// (`wcc_core::store::app_root`).
pub fn root() -> Result<PathBuf, String> {
    wcc_core::store::app_root()
}

#[cfg(windows)]
pub fn local_app_data() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "Windows did not report a Local AppData folder".to_owned())
}

impl Shell {
    pub fn new(
        root: PathBuf,
        started_hidden: bool,
        fake: bool,
        adopt_problems: Vec<String>,
    ) -> Self {
        let settings = ShellSettings::load(&root.join("settings.json"));
        Self {
            root,
            started_hidden,
            fake,
            adopt_problems,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    /// `windows`, `linux` or `macos`: which modules and words the page uses.
    platform: wcc_core::system::Platform,
    elevated: bool,
    test_machine: bool,
    data_dir: String,
    cleaners_dir: String,
    windows_build: Option<u32>,
    /// Started at sign-in: the page stays hidden until opened from the tray.
    started_hidden: bool,
    /// What could not be copied from the earlier apps at this start.
    adopt_problems: Vec<String>,
}

#[tauri::command]
pub fn app_info(app: AppHandle, shell: ShellState<'_>, disk: Disk<'_>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        platform: wcc_core::system::Platform::current(),
        elevated: elevated(&app, &disk),
        test_machine: shell.fake,
        data_dir: shell.root.display().to_string(),
        cleaners_dir: disk.cleaners_dir().display().to_string(),
        windows_build: windows_build(&app),
        started_hidden: shell.started_hidden,
        adopt_problems: shell.adopt_problems.clone(),
    }
}

/// One answer for the whole app, from the registry module's machine on
/// Windows (every module's machine agrees on a real PC).
#[cfg(windows)]
fn elevated(app: &AppHandle, disk: &disk::state::AppState) -> bool {
    app.try_state::<Arc<registry::state::AppState>>()
        .map_or_else(
            || disk.system.is_elevated(),
            |registry| registry.system.is_elevated(),
        )
}

#[cfg(not(windows))]
fn elevated(_app: &AppHandle, disk: &disk::state::AppState) -> bool {
    disk.system.is_elevated()
}

#[cfg(windows)]
fn windows_build(app: &AppHandle) -> Option<u32> {
    let registry = app.try_state::<Arc<registry::state::AppState>>()?;
    wrc_core::tuner::windows_build(registry.system.as_ref())
}

#[cfg(not(windows))]
fn windows_build(_app: &AppHandle) -> Option<u32> {
    None
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
    if removing(&app) {
        return Err(REMOVING.into());
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
    if removing(&app) {
        return Err(REMOVING.into());
    }
    let exe = std::env::current_exe()
        .map_err(|error| CommandError::from(format!("Could not find this program: {error}")))?;
    relaunch(&exe)?;
    app.exit(0);
    Ok(())
}

/// `--after <process id>`: started by "Restart as administrator", whose
/// copy is still closing. Without waiting for it, the single-instance check
/// would hand over to that closing copy and quit, leaving nothing running.
/// Windows only: Linux and macOS never restart the app as administrator.
#[cfg(windows)]
pub const AFTER_FLAG: &str = "--after";

/// The process this copy must wait for, from `--after <id>`.
#[cfg(windows)]
pub fn restarted_from(args: &[String]) -> Option<u32> {
    let at = args.iter().position(|arg| arg == AFTER_FLAG)?;
    args.get(at + 1)?.parse().ok()
}

#[cfg(windows)]
fn relaunch(exe: &std::path::Path) -> CommandResult<()> {
    registry::platform::relaunch_elevated(exe)
}

/// Linux and macOS ask for administrator rights for each task that needs
/// them (polkit, or macOS's own prompt), so the whole app never restarts
/// as root.
#[cfg(not(windows))]
fn relaunch(_exe: &std::path::Path) -> CommandResult<()> {
    Err("WattClean asks for administrator rights when a task needs them.".into())
}

const REMOVING: &str = "Wait for the registry clean or the uninstall's removal to finish first.";

/// A registry clean, or an uninstall's removal or restore, is under way:
/// cut short, its backup would hold entries that were never removed.
#[cfg(windows)]
fn removing(app: &AppHandle) -> bool {
    app.try_state::<Arc<registry::state::AppState>>()
        .is_some_and(|state| state.is_cleaning())
        || app
            .try_state::<Arc<crate::uninstall::state::AppState>>()
            .is_some_and(|state| state.is_removing())
}

/// A package manager is removing a program: cut short, it could leave the
/// package database half-changed.
#[cfg(not(windows))]
fn removing(app: &AppHandle) -> bool {
    app.try_state::<Arc<crate::packages::AppState>>()
        .is_some_and(|state| state.is_removing())
}

/// Quits once the app is idle (`wait_until_idle`), at once when it already is.
pub fn quit_when_idle(app: &AppHandle) {
    if !disk_busy(app) && !removing(app) {
        app.exit(0);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        wait_until_idle(&app);
        app.exit(0);
    });
}

/// Stops the running disk job (a wipe gives its space back when stopped)
/// and waits for it, at most a minute, and for any registry clean or
/// uninstall removal to finish. macOS's own Quit (Cmd+Q, the Dock) ends the
/// app without asking, so the Exit event waits here too, unless the quit
/// already waited.
pub fn wait_until_idle(app: &AppHandle) {
    static WAITED: AtomicBool = AtomicBool::new(false);
    if WAITED.load(Ordering::SeqCst) {
        return;
    }
    if disk_busy(app)
        && let Some(disk) = app.try_state::<Arc<disk::state::AppState>>()
    {
        disk.cancel();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while (disk_busy(app) && std::time::Instant::now() < deadline) || removing(app) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    WAITED.store(true, Ordering::SeqCst);
}

fn disk_busy(app: &AppHandle) -> bool {
    app.try_state::<Arc<disk::state::AppState>>()
        .is_some_and(|disk| disk.is_busy())
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn a_restarted_copy_knows_whom_to_wait_for() {
        let args = |text: &str| -> Vec<String> { text.split(' ').map(str::to_string).collect() };
        assert_eq!(
            restarted_from(&args("wattclean.exe --after 4242")),
            Some(4242)
        );
        assert_eq!(restarted_from(&args("wattclean.exe --after")), None);
        assert_eq!(restarted_from(&args("wattclean.exe --after x")), None);
        assert_eq!(restarted_from(&args("wattclean.exe --background")), None);
    }
}
