//! One uninstall from start to finish: what is known about the program,
//! its uninstaller while it runs, the scan, the removal, and the record.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use wu_core::backup::{self, Removal};
use wu_core::folders::Folders;
use wu_core::history::{self, Counts, Uninstaller};
use wu_core::identity::Identity;
use wu_core::install_log::{InstallLog, Recorded};
use wu_core::programs::list_programs;
use wu_core::scan::{self, Context, Mode, OtherProgram, Progress, ScanReport};
use wu_core::{Program, Running, Scope, System};

use crate::error::{CommandError, CommandResult};
use crate::uninstall::state::AppState;

/// `ERROR_ELEVATION_REQUIRED` and `ERROR_SERVICE_DISABLED`.
const ELEVATION_REQUIRED: i32 = 740;
const SERVICE_DISABLED: i32 = 1058;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[derive(Default)]
pub struct ScanProgress {
    area: Mutex<String>,
    cancel: AtomicBool,
}

impl ScanProgress {
    pub fn current(&self) -> String {
        self.area
            .lock()
            .map(|area| area.clone())
            .unwrap_or_default()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Progress for ScanProgress {
    fn area(&self, area: &str) {
        if let Ok(mut current) = self.area.lock() {
            area.clone_into(&mut current);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct Mutable {
    running: Option<Box<dyn Running>>,
    uninstaller: Option<Uninstaller>,
    report: Option<ScanReport>,
    recorded: bool,
}

pub struct Session {
    pub id: u32,
    pub program: Option<Program>,
    pub identity: Identity,
    /// No uninstaller is run: the leftovers are found and removed directly.
    pub forced: bool,
    /// The program's install log, when it was installed with tracing.
    pub log: Option<InstallLog>,
    pub progress: ScanProgress,
    state: Mutex<Mutable>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: u32,
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub forced: bool,
    pub install_dirs: Vec<String>,
    pub executables: usize,
    pub uninstall_command: Option<String>,
    pub needs_admin: bool,
    /// Items the install log will check, when there is one.
    pub logged_items: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedItem {
    pub item: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalView {
    pub removed: Counts,
    pub at_restart: u32,
    pub failed: Vec<FailedItem>,
    pub bytes: u64,
    pub backup_id: String,
}

impl Session {
    pub fn new(
        id: u32,
        program: Option<Program>,
        identity: Identity,
        forced: bool,
        log: Option<InstallLog>,
    ) -> Self {
        Self {
            id,
            program,
            identity,
            forced,
            log,
            progress: ScanProgress::default(),
            state: Mutex::new(Mutable::default()),
        }
    }

    fn state(&self) -> MutexGuard<'_, Mutable> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn view(&self, system: &dyn System) -> SessionView {
        let program = self.program.as_ref();
        SessionView {
            id: self.id,
            name: self.identity.display_name.clone(),
            publisher: self.identity.publisher.clone(),
            version: program.and_then(|p| p.version.clone()),
            forced: self.forced,
            install_dirs: self
                .identity
                .install_dirs
                .iter()
                .map(|d| d.display().to_string())
                .collect(),
            executables: self.identity.exe_names.len(),
            uninstall_command: program.and_then(|p| p.uninstall_command.clone()),
            needs_admin: !system.is_elevated() && program.is_none_or(|p| p.scope != Scope::User),
            logged_items: self.log.as_ref().map(|log| {
                log.items
                    .iter()
                    .filter(|item| item.recorded == Recorded::Created)
                    .count()
            }),
        }
    }

    pub fn create_restore_point(&self, system: &dyn System) -> CommandResult<()> {
        let description = format!("Before uninstalling {}", self.identity.display_name);
        system.create_restore_point(&description).map_err(|error| {
            let reason = if error.raw_os_error() == Some(SERVICE_DISABLED) {
                "System Restore is turned off on this PC".to_owned()
            } else {
                error.to_string()
            };
            CommandError::from(format!("No restore point was made: {reason}."))
        })
    }

    pub fn run_uninstaller(&self, system: &dyn System, quiet: bool) -> CommandResult<String> {
        let program = self
            .program
            .as_ref()
            .ok_or("A forced uninstall has no uninstaller to run.")?;
        let folders = Folders::load(system)?;
        let launch = wu_core::command_line::uninstall_launch(system, &folders, program, quiet)?;
        match system.launch(&launch) {
            Ok(running) => {
                let mut state = self.state();
                state.running = Some(running);
                state.uninstaller = None;
                Ok(launch.display())
            }
            Err(error) => {
                let message = if error.raw_os_error() == Some(ELEVATION_REQUIRED) {
                    "This uninstaller needs administrator rights. Restart WattClean as administrator and try again.".to_owned()
                } else if error.kind() == std::io::ErrorKind::NotFound {
                    format!(
                        "The uninstaller is missing ({}). You can still scan for leftovers.",
                        launch.program.display()
                    )
                } else {
                    format!("The uninstaller did not start: {error}")
                };
                self.state().uninstaller = Some(Uninstaller::Failed {
                    error: message.clone(),
                });
                Err(message.into())
            }
        }
    }

    /// True once the uninstaller and every process it started have exited.
    pub fn uninstaller_finished(&self) -> CommandResult<bool> {
        let mut state = self.state();
        let Some(running) = state.running.as_mut() else {
            return Ok(true);
        };
        let finished = running.finished().map_err(|error| {
            CommandError::from(format!("Could not follow the uninstaller: {error}"))
        })?;
        if finished {
            state.running = None;
            state.uninstaller = Some(Uninstaller::Finished);
        }
        Ok(finished)
    }

    pub fn scan(&self, app: &AppState, mode: Mode) -> CommandResult<ScanReport> {
        let system = app.system.as_ref();
        let folders = Folders::load(system)?;
        let programs = list_programs(system, true);
        let target = self.program.as_ref();
        let others = OtherProgram::all(system, &folders, &programs, target);
        let context = Context {
            system,
            folders: &folders,
            others: &others,
            progress: &self.progress,
            log: self.log.as_ref(),
            protected: &app.protected,
        };
        let report = scan::scan(&self.identity, mode, &context);
        release_hives(system);
        if self.progress.cancelled() {
            return Err("The scan was cancelled.".into());
        }
        self.state().report = Some(report.clone());
        Ok(report)
    }

    pub fn remove(&self, app: &AppState, ids: &[u32]) -> CommandResult<RemovalView> {
        let report = self
            .state()
            .report
            .clone()
            .ok_or("Scan for leftovers first.")?;
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        let mut chosen = Vec::new();
        for id in &ids {
            let item = report.items.iter().find(|item| item.id == *id);
            chosen.push(
                item.cloned()
                    .ok_or("The list of leftovers changed; scan again.")?,
            );
        }
        let removal = backup::remove(
            app.system.as_ref(),
            &app.backups_dir(),
            &self.identity.display_name,
            &chosen,
            now(),
        );
        release_hives(app.system.as_ref());
        let removal = removal?;
        self.record(app, Some(report.mode), Some(&removal))?;
        Ok(RemovalView {
            removed: removal.removed,
            at_restart: removal.at_restart,
            failed: removal
                .failed
                .into_iter()
                .map(|(item, error)| FailedItem { item, error })
                .collect(),
            bytes: removal.bytes,
            backup_id: removal.backup.id,
        })
    }

    /// Write the history entry once: after a removal, or when the wizard
    /// closes after an uninstaller ran with nothing removed.
    pub fn record(
        &self,
        app: &AppState,
        mode: Option<Mode>,
        removal: Option<&Removal>,
    ) -> CommandResult<()> {
        let mut state = self.state();
        if state.recorded {
            return Ok(());
        }
        let uninstaller = match (&self.program, &state.uninstaller) {
            (None, _) => Uninstaller::NotRun,
            (Some(_), Some(outcome)) => outcome.clone(),
            (Some(_), None) if removal.is_none() => return Ok(()),
            (Some(_), None) => Uninstaller::NotRun,
        };
        let program = self.program.as_ref();
        let entry = history::Entry {
            id: format!("{}-{}", now(), self.id),
            time: now(),
            program: self.identity.display_name.clone(),
            publisher: self.identity.publisher.clone(),
            version: program.and_then(|p| p.version.clone()),
            mode,
            uninstaller,
            removed: removal.map(|r| r.removed).unwrap_or_default(),
            at_restart: removal.map_or(0, |r| r.at_restart),
            failed: removal.map_or(0, |r| u32::try_from(r.failed.len()).unwrap_or(u32::MAX)),
            bytes: removal.map_or(0, |r| r.bytes),
            backup_id: removal.map(|r| r.backup.id.clone()),
        };
        history::append(&app.data_dir, entry)?;
        state.recorded = true;
        Ok(())
    }
}

/// Validate a forced uninstall's folder: absolute, existing, a folder.
pub fn forced_folder(text: &str, folders: &Folders) -> CommandResult<Option<PathBuf>> {
    let text = text.trim().trim_matches('"');
    if text.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(text);
    if !path.is_absolute() || !path.is_dir() {
        return Err(
            "Choose an existing folder, written as a full path such as C:\\Program Files\\App."
                .into(),
        );
    }
    // Everything registered under the folder would be ticked for removal, so
    // Windows' folders, the drive root and the shared roots are refused.
    if !Identity::is_own_folder(&path, folders) {
        return Err(
            "That folder is shared by Windows or other programs. Choose the program's own folder, such as C:\\Program Files\\App, or leave it empty."
                .into(),
        );
    }
    Ok(Some(path))
}

/// The program's install log, if it was installed with tracing. A program
/// whose entry names no folder gets the folders the log saw it create.
pub fn attach_log(
    data_dir: &std::path::Path,
    folders: &Folders,
    program: &Program,
    identity: &mut Identity,
) -> Option<InstallLog> {
    let log = wu_core::install_log::for_program(data_dir, program)?;
    if identity.install_dirs.is_empty() {
        identity.install_dirs = log.install_dirs(folders);
        identity.read_executables();
    }
    Some(log)
}

/// Unload other accounts' hives after each phase; a failure here is a
/// Windows error worth seeing, but never a reason to lose the phase's result.
pub fn release_hives(system: &dyn System) {
    if let Err(error) = system.release_account_hives() {
        eprintln!("could not unload an account's registry hive: {error}");
    }
}
