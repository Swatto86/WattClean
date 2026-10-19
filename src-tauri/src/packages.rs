//! The uninstaller on Linux and macOS: the programs the package managers
//! and Applications folders hold, removing one with its package manager,
//! and moving what it left behind to the Trash. The page names a program
//! by the key it was listed under and leftovers by number; every path and
//! command is decided here and in `wu_core::packages`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::State;
use wu_core::packages::{self, Leftover, Package, PackageSystem, Removal};

use crate::error::{CommandError, CommandResult, blocking};

pub struct AppState {
    system: Arc<dyn PackageSystem>,
    /// Everything installed, as last listed (applications and the rest).
    listed: Mutex<Vec<Package>>,
    /// The last removal's leftovers, numbered by position.
    leftovers: Mutex<Vec<Leftover>>,
    removing: AtomicBool,
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
        }
    }

    pub fn is_removing(&self) -> bool {
        self.removing.load(Ordering::SeqCst)
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
pub struct LeftoverView {
    number: usize,
    path: String,
    size: u64,
    folder: bool,
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
/// for what it left behind in the home folder.
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
        if shared.removing.swap(true, Ordering::SeqCst) {
            return Err("Another program is being removed: wait for it to finish.".into());
        }
        let _removing = Removing(&shared.removing);
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
        let views = found
            .iter()
            .enumerate()
            .map(|(number, leftover)| LeftoverView {
                number,
                path: leftover.path.display().to_string(),
                size: leftover.size,
                folder: leftover.folder,
            })
            .collect();
        *lock(&shared.leftovers) = found;
        Ok(Uninstalled {
            name: package.name,
            said,
            leftovers: views,
        })
    })
    .await
}

/// Carries out a removal; returns the package manager's last words.
fn remove(
    system: &dyn PackageSystem,
    package: &Package,
    purge: bool,
) -> CommandResult<Option<String>> {
    match packages::removal(system, package, purge)? {
        Removal::Command {
            program,
            args,
            admin,
        } => {
            let output = system.run(&program, &args, admin)?;
            let said = output
                .stderr
                .lines()
                .chain(output.stdout.lines())
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .map(str::to_string);
            if output.success {
                Ok(said)
            } else {
                Err(CommandError::from(format!(
                    "{} could not be removed: {}",
                    package.name,
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
                    CommandError::from(format!(
                        "{} could not be moved to the Trash: {error}",
                        package.name
                    ))
                })
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trashed {
    moved: usize,
    failed: Vec<String>,
}

/// Moves the chosen leftovers (by number) of the last removal to the Trash.
#[tauri::command]
pub async fn un_pkg_trash_leftovers(state: App<'_>, numbers: Vec<usize>) -> CommandResult<Trashed> {
    let shared = Arc::clone(&state);
    blocking(move || {
        let chosen: Vec<std::path::PathBuf> = {
            let leftovers = lock(&shared.leftovers);
            let mut numbers = numbers;
            numbers.sort_unstable();
            numbers.dedup();
            numbers
                .iter()
                .map(|n| leftovers.get(*n).map(|l| l.path.clone()))
                .collect::<Option<_>>()
                .ok_or("Those leftovers are no longer listed.")?
        };
        let mut moved = 0;
        let mut failed = Vec::new();
        for (path, result) in chosen.iter().zip(shared.system.trash(&chosen)) {
            match result {
                Ok(()) => moved += 1,
                Err(error) => failed.push(format!("{}: {error}", path.display())),
            }
        }
        lock(&shared.leftovers).clear();
        Ok(Trashed { moved, failed })
    })
    .await
}
