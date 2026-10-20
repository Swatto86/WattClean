//! What a removal (or the search for removed apps' leftovers, on macOS)
//! found: shown to the page by number, revealed in the file manager, and
//! moved to the Trash, a background job stopped first.

use std::sync::Arc;

use serde::Serialize;
use wu_core::packages::{self, Leftover, PackageSystem};

use super::{App, lock, run_step};
use crate::error::{CommandError, CommandResult, blocking};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverView {
    number: usize,
    path: String,
    /// `None` when its size could not be read.
    size: Option<u64>,
    folder: bool,
    /// A background job's plist or helper, stopped before it goes to the
    /// Trash.
    job: bool,
    /// In `/Library`, for every account: moving it asks for an
    /// administrator's password.
    system_wide: bool,
}

/// Leftovers as the page shows them, numbered by position.
pub(super) fn views(leftovers: &[Leftover]) -> Vec<LeftoverView> {
    leftovers
        .iter()
        .enumerate()
        .map(|(number, leftover)| LeftoverView {
            number,
            path: leftover.path.display().to_string(),
            size: leftover.size,
            folder: leftover.folder,
            job: leftover.job.is_some(),
            system_wide: leftover.system_wide,
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Orphans {
    leftovers: Vec<LeftoverView>,
    /// Whether Spotlight could be asked; without it only jobs whose app
    /// folder is gone are found.
    spotlight: bool,
}

/// The background jobs of apps that are gone, and the helpers their
/// daemons start (macOS): held for the Trash as a removal's leftovers are.
/// Marked as a removal while it looks, since its list replaces the one a
/// removal's numbers name.
#[tauri::command]
pub async fn un_pkg_orphans(state: App<'_>) -> CommandResult<Orphans> {
    let shared = Arc::clone(&state);
    blocking(move || {
        let _looking = shared.begin_removal()?;
        let (installed, _) = shared.list_all();
        let found = packages::orphans(shared.system.as_ref(), &installed);
        let leftovers = views(&found.leftovers);
        *lock(&shared.leftovers) = found.leftovers;
        Ok(Orphans {
            leftovers,
            spotlight: found.spotlight,
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trashed {
    pub(super) moved: usize,
    pub(super) failed: Vec<String>,
}

/// Moves the chosen leftovers (by number) of the last removal, or of the
/// last search for removed apps' ones, to the Trash.
#[tauri::command]
pub async fn un_pkg_trash_leftovers(state: App<'_>, numbers: Vec<usize>) -> CommandResult<Trashed> {
    let shared = Arc::clone(&state);
    blocking(move || {
        let chosen: Vec<Leftover> = {
            let leftovers = lock(&shared.leftovers);
            let mut numbers = numbers;
            numbers.sort_unstable();
            numbers.dedup();
            numbers
                .iter()
                .map(|n| leftovers.get(*n).cloned())
                .collect::<Option<_>>()
                .ok_or("Those leftovers are no longer listed.")?
        };
        let trashed = trash_chosen(shared.system.as_ref(), &chosen);
        lock(&shared.leftovers).clear();
        Ok(trashed)
    })
    .await
}

/// Moves `chosen` to the Trash. A plist or helper whose background job
/// launchd has loaded is stopped first (a trashed plist's job runs on until
/// logout); one that cannot be stopped stays, and is reported.
pub(super) fn trash_chosen(system: &dyn PackageSystem, chosen: &[Leftover]) -> Trashed {
    let mut failed = Vec::new();
    let mut paths = Vec::new();
    for leftover in chosen {
        let stopped = match packages::stop_job(system, leftover) {
            Ok(None) => Ok(()),
            Ok(Some(step)) => run_step(system, &step)
                .map_err(|said| format!("its background job could not be stopped: {said}")),
            Err(refused) => Err(refused),
        };
        match stopped {
            Ok(()) => paths.push(leftover.path.clone()),
            Err(why) => failed.push(format!("{}: {why}", leftover.path.display())),
        }
    }
    let mut moved = 0;
    for (path, result) in paths.iter().zip(system.trash(&paths)) {
        match result {
            Ok(()) => moved += 1,
            Err(error) => failed.push(format!("{}: {error}", path.display())),
        }
    }
    Trashed { moved, failed }
}

/// Shows one leftover (by number) of the last removal in Finder, or the
/// folder holding it in the file manager.
#[tauri::command]
pub async fn un_pkg_reveal(state: App<'_>, number: usize) -> CommandResult<()> {
    let shared = Arc::clone(&state);
    blocking(move || {
        // Not held while the file manager is asked: it may be slow to answer.
        let leftovers = lock(&shared.leftovers).clone();
        reveal(shared.system.as_ref(), &leftovers, number)
    })
    .await
}

pub(super) fn reveal(
    system: &dyn PackageSystem,
    leftovers: &[Leftover],
    number: usize,
) -> CommandResult<()> {
    let path = &leftovers
        .get(number)
        .ok_or("That leftover is no longer listed.")?
        .path;
    system.reveal(path).map_err(|error| {
        CommandError::from(format!("{} could not be shown: {error}", path.display()))
    })
}
