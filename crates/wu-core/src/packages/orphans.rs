//! Leftovers of removed apps (macOS): the background jobs of apps that are
//! gone however they went (dragged to the Trash, removed by another tool),
//! from the account's LaunchAgents and `/Library`'s two folders of jobs. A
//! job is listed only when the apps its `AssociatedBundleIdentifiers` names
//! are neither listed nor found by Spotlight, or when the program it starts
//! lay inside an app bundle that no longer exists. A job that merely points
//! at a missing program is not one: a tool's path changes when it is
//! upgraded (Homebrew's PowerShell). The helpers their daemons start go
//! with them, as `daemons` decides. Apple's and WattClean's own jobs are
//! never listed, and nothing is pre-selected.

use std::path::{Component, Path, PathBuf};

use super::daemons::{self, Plist};
use super::launchd::{LaunchdJob, is_own};
use super::leftovers::{Leftover, tidy};
use super::{Package, PackageSystem};

/// What the search found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Orphans {
    pub leftovers: Vec<Leftover>,
    /// Whether Spotlight could be asked; without it a job is judged by its
    /// program alone.
    pub spotlight: bool,
}

/// The background jobs whose app is gone, and their helpers; `installed`
/// is everything listed.
pub fn orphans(system: &dyn PackageSystem, installed: &[Package]) -> Orphans {
    let Some(home) = system.home().filter(|_| system.is_macos()) else {
        return Orphans::default();
    };
    let listed: Vec<String> = installed
        .iter()
        .filter_map(|package| package.bundle_id.as_deref())
        .map(str::to_lowercase)
        .collect();
    let spotlight = Spotlight::ask(system, installed);
    let all = daemons::plists(system, &home);
    let gone: Vec<&Plist> = all
        .iter()
        .filter(|plist| {
            plist
                .job()
                .is_some_and(|job| orphaned(system, job, &listed, spotlight.as_ref()))
        })
        .collect();
    let mut leftovers: Vec<Leftover> = gone.iter().filter_map(|plist| plist.leftover()).collect();
    leftovers.extend(daemons::helpers(system, &gone, &all));
    Orphans {
        leftovers: tidy(leftovers, None),
        spotlight: spotlight.is_some(),
    }
}

/// Whether `job`'s app is gone. `listed` holds the listed apps' bundle
/// identifiers, in lower case.
fn orphaned(
    system: &dyn PackageSystem,
    job: &LaunchdJob,
    listed: &[String],
    spotlight: Option<&Spotlight>,
) -> bool {
    let label = job.label.to_lowercase();
    if is_own(&label) || label.starts_with("com.apple.") {
        return false;
    }
    if job
        .associated
        .iter()
        .any(|id| listed.contains(&id.to_lowercase()))
    {
        return false;
    }
    let app_gone = match job.program.as_deref().and_then(app_holding) {
        Some(app) => match std::fs::symlink_metadata(system.at(&app.to_string_lossy())) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            // There, or it cannot be told.
            _ => return false,
        },
        None => false,
    };
    match spotlight {
        Some(spotlight) if !job.associated.is_empty() => {
            job.associated.iter().all(|id| spotlight.lacks(id))
        }
        _ => app_gone,
    }
}

/// The outermost app bundle holding `program` (`/Applications/Foo.app` for
/// a helper app inside it); none on another disk (`/Volumes/…`, which may
/// only be unplugged) or for a path that is not plain.
fn app_holding(program: &str) -> Option<PathBuf> {
    let mut held = PathBuf::new();
    for part in Path::new(program).components() {
        match part {
            Component::RootDir if held.as_os_str().is_empty() => held.push("/"),
            Component::Normal(name) if !held.as_os_str().is_empty() => {
                held.push(name);
                let name = name.to_string_lossy().to_lowercase();
                if name.len() > 4 && name.ends_with(".app") {
                    let elsewhere = held
                        .components()
                        .nth(1)
                        .is_some_and(|top| top.as_os_str().eq_ignore_ascii_case("Volumes"));
                    return (!elsewhere).then_some(held);
                }
            }
            _ => return None,
        }
    }
    None
}

/// Spotlight, asked for apps by bundle identifier.
struct Spotlight<'a> {
    system: &'a dyn PackageSystem,
    mdfind: PathBuf,
}

impl<'a> Spotlight<'a> {
    /// Spotlight, when it finds an app known to be there (the first listed
    /// app with a bundle identifier, else Finder): an empty answer from an
    /// index that is off would make every app look gone.
    fn ask(system: &'a dyn PackageSystem, installed: &[Package]) -> Option<Self> {
        let spotlight = Self {
            system,
            mdfind: system.find_program("mdfind")?,
        };
        let known = installed
            .iter()
            .filter_map(|package| package.bundle_id.as_deref())
            .find(|id| looks_like_id(id))
            .unwrap_or("com.apple.finder");
        (spotlight.finds(known) == Some(true)).then_some(spotlight)
    }

    /// Whether Spotlight finds an app with this bundle identifier outside
    /// a Trash; `None` when it cannot be asked about it.
    fn finds(&self, id: &str) -> Option<bool> {
        if !looks_like_id(id) {
            return None;
        }
        let query = format!("kMDItemCFBundleIdentifier == \"{id}\"c");
        let output = self.system.run(&self.mdfind, &[query], false, &[]).ok()?;
        output.success.then(|| {
            output
                .stdout
                .lines()
                .map(str::trim)
                .any(|path| !path.is_empty() && !in_trash(path))
        })
    }

    /// Whether Spotlight is sure no such app is there.
    fn lacks(&self, id: &str) -> bool {
        self.finds(id) == Some(false)
    }
}

/// A bundle identifier Spotlight's query can hold as it is.
fn looks_like_id(id: &str) -> bool {
    id.len() <= 255
        && id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn in_trash(path: &str) -> bool {
    path.contains("/.Trash/") || path.contains("/.Trashes/")
}

#[cfg(test)]
#[path = "orphans_tests.rs"]
mod tests;
