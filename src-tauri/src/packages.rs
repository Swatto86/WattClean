//! The uninstaller on Linux and macOS: the programs the package managers
//! and Applications folders hold, removing one with its package manager,
//! and moving what it left behind to the Trash, as well as (on macOS) the
//! background jobs of apps already gone. The page names a program by the
//! key it was listed under and leftovers by number; every path and command
//! is decided here and in `wu_core::packages`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::State;
use wu_core::packages::{self, Leftover, Package, PackageSystem, Removal, Step};

use crate::error::{CommandError, CommandResult, blocking};

pub mod leftovers;

use leftovers::{LeftoverView, views};
#[cfg(test)]
use leftovers::{reveal, trash_chosen};

pub struct AppState {
    system: Arc<dyn PackageSystem>,
    /// Everything installed, as last listed (applications and the rest).
    listed: Mutex<Vec<Package>>,
    /// The last removal's leftovers (or removed apps' ones), numbered by
    /// position.
    leftovers: Mutex<Vec<Leftover>>,
    removing: AtomicBool,
    /// Set once the app has begun quitting: a removal started now could be
    /// cut short when it exits.
    quitting: AtomicBool,
}

type App<'a> = State<'a, Arc<AppState>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Each list is replaced whole, so a panic elsewhere leaves it usable.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Clears the removing flag however the removal ends.
struct Removing<'a>(&'a AtomicBool);

impl Drop for Removing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl AppState {
    pub fn new(system: Arc<dyn PackageSystem>) -> Self {
        Self {
            system,
            listed: Mutex::new(Vec::new()),
            leftovers: Mutex::new(Vec::new()),
            removing: AtomicBool::new(false),
            quitting: AtomicBool::new(false),
        }
    }

    pub fn is_removing(&self) -> bool {
        self.removing.load(Ordering::SeqCst)
    }

    /// From now on no removal starts: the app is quitting.
    pub fn stop_new_removals(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    /// Marks a removal under way until the guard is dropped; refused while
    /// another runs or the app is quitting.
    fn begin_removal(&self) -> CommandResult<Removing<'_>> {
        if self.removing.swap(true, Ordering::SeqCst) {
            return Err("Another program is being removed: wait for it to finish.".into());
        }
        let removing = Removing(&self.removing);
        // Looked at once marked, so a quit beginning meanwhile either waits
        // for this removal or is seen here.
        if self.quitting.load(Ordering::SeqCst) {
            return Err("WattClean is quitting: it closes once the running task ends.".into());
        }
        Ok(removing)
    }

    fn list_all(&self) -> (Vec<Package>, Vec<String>) {
        let (packages, problems) = packages::list(self.system.as_ref(), true);
        lock(&self.listed).clone_from(&packages);
        (packages, problems)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageView {
    key: String,
    name: String,
    version: Option<String>,
    size: Option<u64>,
    summary: Option<String>,
    app: bool,
    /// Where it came from: `APT`, `Flatpak`, `Homebrew Cask`…
    source: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageList {
    packages: Vec<PackageView>,
    /// Package managers that could not list what they installed.
    problems: Vec<String>,
}

/// What the machine has installed; applications only unless `all`.
#[tauri::command]
pub async fn un_pkg_list(state: App<'_>, all: bool) -> CommandResult<PackageList> {
    let shared = Arc::clone(&state);
    blocking(move || {
        let (packages, problems) = shared.list_all();
        let packages = packages
            .into_iter()
            .filter(|package| all || package.app)
            .map(|package| PackageView {
                key: package.key(),
                source: package.manager.label(),
                name: package.name,
                version: package.version,
                size: package.size,
                summary: package.summary,
                app: package.app,
            })
            .collect();
        Ok(PackageList { packages, problems })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Uninstalled {
    name: String,
    /// What the package manager said last, when it said anything.
    said: Option<String>,
    leftovers: Vec<LeftoverView>,
}

/// Removes one listed program (with its settings when `purge`), then looks
/// for what it left behind: in the home folder, and on macOS its
/// background jobs and helpers in `/Library`. An app still open is refused
/// before anything is removed.
#[tauri::command]
pub async fn un_pkg_uninstall(
    state: App<'_>,
    key: String,
    purge: bool,
) -> CommandResult<Uninstalled> {
    let shared = Arc::clone(&state);
    blocking(move || {
        let package = lock(&shared.listed)
            .iter()
            .find(|package| package.key() == key)
            .cloned()
            .ok_or("That program is no longer listed: refresh the list.")?;
        let _removing = shared.begin_removal()?;
        let said = remove(shared.system.as_ref(), &package, purge)?;
        let (installed, _) = shared.list_all();
        if installed.iter().any(|other| other.key() == package.key()) {
            return Err(CommandError::from(format!(
                "{} is still installed{}",
                package.name,
                said.as_deref()
                    .map_or_else(String::new, |said| format!(": {said}"))
            )));
        }
        let found = packages::find_leftovers(shared.system.as_ref(), &package, &installed);
        let leftovers = views(&found);
        *lock(&shared.leftovers) = found;
        Ok(Uninstalled {
            name: package.name,
            said,
            leftovers,
        })
    })
    .await
}

/// Removes `package`; returns the package manager's last words.
fn remove(
    system: &dyn PackageSystem,
    package: &Package,
    purge: bool,
) -> CommandResult<Option<String>> {
    let plan = packages::removal(system, package, purge)?;
    carry_out(system, &package.name, plan)
}

/// Carries out a planned removal of the program called `name`: the steps
/// before it in order (stopping its background service), then the package
/// manager, which on macOS asks for Homebrew's passwords through
/// WattClean's password box. Nothing more runs once a step fails.
fn carry_out(
    system: &dyn PackageSystem,
    name: &str,
    plan: Removal,
) -> CommandResult<Option<String>> {
    match plan {
        Removal::Command {
            program,
            args,
            admin,
            before,
            askpass,
        } => {
            for step in &before {
                run_step(system, step).map_err(|said| {
                    format!(
                        "{name}'s background service could not be stopped, so it was left installed: {said}"
                    )
                })?;
            }
            let helper = if askpass {
                let helper = system.askpass().map_err(|error| {
                    format!(
                        "WattClean could not prepare the password box Homebrew may need, so {name} was left installed: {error}"
                    )
                })?;
                let text = helper.to_str().map(str::to_string);
                Some(text.ok_or("WattClean's data folder has a name sudo cannot be given")?)
            } else {
                None
            };
            let env: Vec<(&str, &str)> = helper
                .as_deref()
                .map(|helper| ("SUDO_ASKPASS", helper))
                .into_iter()
                .collect();
            let output = system.run(&program, &args, admin, &env)?;
            // Its error, not the progress it printed before failing.
            let said = output.last_words().map(str::to_string);
            if output.success {
                Ok(said)
            } else {
                Err(CommandError::from(format!(
                    "{name} could not be removed: {}",
                    said.as_deref()
                        .unwrap_or("the package manager reported a failure")
                )))
            }
        }
        Removal::Trash(path) => {
            let result = system.trash(&[path]).into_iter().next();
            result
                .unwrap_or_else(|| Err(std::io::Error::other("nothing was moved")))
                .map(|()| None)
                .map_err(|error| {
                    CommandError::from(format!("{name} could not be moved to the Trash: {error}"))
                })
        }
    }
}

/// Runs one step of a removal; what it said when it failed.
fn run_step(system: &dyn PackageSystem, step: &Step) -> Result<(), String> {
    let output = system
        .run(&step.program, &step.args, step.admin, &[])
        .map_err(|error| error.to_string())?;
    if output.success {
        Ok(())
    } else {
        let said = output.last_words().unwrap_or("it reported a failure");
        Err(said.to_string())
    }
}

#[cfg(test)]
#[path = "packages_tests.rs"]
mod tests;
