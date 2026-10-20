//! The background jobs a removed app leaves for every account (macOS):
//! launchd's plists in `/Library/LaunchAgents` (run in each account's
//! session) and `/Library/LaunchDaemons` (run as root), and the helper
//! programs in `/Library/PrivilegedHelperTools` its daemons start. Outside
//! the home folder no name is enough: a plist is the app's only when its
//! `AssociatedBundleIdentifiers` holds the app's bundle identifier, whole
//! and in the same letters, or the program it starts lies inside the
//! removed bundle; a helper only when it sits directly in that folder, a
//! daemon of the app starts it and no other plist refers to it. Moving
//! them asks for an administrator's password (Finder asks), and a daemon
//! is stopped as administrator first (`remove::stop_job`).

use std::path::{Component, Path, PathBuf};

use super::launchd::{Domain, Job, LaunchdJob, is_own};
use super::leftovers::{Leftover, leftover};
use super::{Package, PackageSystem, within};

const AGENTS: &str = "/Library/LaunchAgents";
const DAEMONS: &str = "/Library/LaunchDaemons";
const HELPERS: &str = "/Library/PrivilegedHelperTools";

/// A plist in one of launchd's folders.
pub(super) struct Plist {
    path: PathBuf,
    /// What it says; `None` when it is no job launchd would load.
    job: Option<LaunchdJob>,
    /// Whether this account can read it: one it cannot might start anything.
    readable: bool,
    /// A file of its own, not a link to one elsewhere.
    file: bool,
    domain: Domain,
    /// In `/Library`, for every account.
    system_wide: bool,
}

impl Plist {
    pub(super) fn job(&self) -> Option<&LaunchdJob> {
        self.job.as_ref().filter(|_| self.file)
    }

    pub(super) fn leftover(&self) -> Option<Leftover> {
        let job = self.job()?;
        Some(Leftover {
            job: Some(Job::new(&job.label, self.domain)),
            system_wide: self.system_wide,
            ..leftover(&self.path)
        })
    }
}

/// Every plist in the account's LaunchAgents and in `/Library`'s two
/// folders of jobs.
pub(super) fn plists(system: &dyn PackageSystem, home: &Path) -> Vec<Plist> {
    let folders = [
        (
            home.join("Library").join("LaunchAgents"),
            Domain::Session,
            false,
        ),
        (system.at(AGENTS), Domain::Session, true),
        (system.at(DAEMONS), Domain::System, true),
    ];
    let mut out = Vec::new();
    for (folder, domain, system_wide) in folders {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let plist = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("plist"));
            if !plist {
                continue;
            }
            let file = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file());
            out.push(Plist {
                readable: std::fs::File::open(&path).is_ok(),
                job: system.launchd_job(&path),
                path,
                file,
                domain,
                system_wide,
            });
        }
    }
    out
}

/// `removed`'s plists in `/Library` and the helpers its daemons start.
/// `other_bundles` are the other installed apps' bundle identifiers, in
/// lower case: a plist naming one of them stays.
pub(super) fn for_app(
    system: &dyn PackageSystem,
    home: &Path,
    removed: &Package,
    other_bundles: &[String],
) -> Vec<Leftover> {
    let all = plists(system, home);
    let mine: Vec<&Plist> = all
        .iter()
        .filter(|plist| plist.system_wide)
        .filter(|plist| {
            plist
                .job()
                .is_some_and(|job| belongs(job, removed, other_bundles))
        })
        .collect();
    let mut out: Vec<Leftover> = mine.iter().filter_map(|plist| plist.leftover()).collect();
    out.extend(helpers(system, &mine, &all));
    out
}

/// Whether a system-wide job is `removed`'s.
fn belongs(job: &LaunchdJob, removed: &Package, other_bundles: &[String]) -> bool {
    if is_own(&job.label) {
        return false;
    }
    let anothers = job
        .associated
        .iter()
        .any(|id| other_bundles.contains(&id.to_lowercase()));
    let names_app = removed
        .bundle_id
        .as_deref()
        .is_some_and(|id| job.associated.iter().any(|named| named == id));
    let starts_app = removed
        .bundle()
        .zip(job.program.as_deref())
        .is_some_and(|(bundle, program)| within(program, bundle));
    !anothers && (names_app || starts_app)
}

/// The helpers the daemons among `owners` start: a file directly in
/// `/Library/PrivilegedHelperTools` that no plist but the owners' refers
/// to, in any letters, anywhere in it. None while a plist cannot be read.
/// Each goes with its daemon's job, stopped before it moves.
pub(super) fn helpers(
    system: &dyn PackageSystem,
    owners: &[&Plist],
    all: &[Plist],
) -> Vec<Leftover> {
    if all.iter().any(|plist| !plist.readable) {
        return Vec::new();
    }
    let others: Vec<&LaunchdJob> = all
        .iter()
        .filter(|plist| !owners.iter().any(|owner| owner.path == plist.path))
        .filter_map(|plist| plist.job.as_ref())
        .collect();
    owners
        .iter()
        .filter(|owner| owner.domain == Domain::System)
        .filter_map(|owner| {
            let job = owner.job()?;
            let program = job.program.as_deref().filter(|p| is_helper(p))?;
            let path = system.at(program);
            if !std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
                return None;
            }
            let program = program.to_lowercase();
            let used = others.iter().any(|other| {
                other
                    .mentions
                    .iter()
                    .any(|text| text.to_lowercase().contains(&program))
            });
            (!used).then(|| Leftover {
                job: Some(Job::new(&job.label, Domain::System)),
                system_wide: true,
                ..leftover(&path)
            })
        })
        .collect()
}

/// `/Library/PrivilegedHelperTools/<name>`, nothing above or below.
fn is_helper(program: &str) -> bool {
    let mut parts = Path::new(program).components();
    let mut folder = Path::new(HELPERS).components();
    let same = |a: Option<Component>, b: Option<Component>| match (a, b) {
        (Some(Component::RootDir), Some(Component::RootDir)) => true,
        (Some(Component::Normal(a)), Some(Component::Normal(b))) => a.eq_ignore_ascii_case(b),
        _ => false,
    };
    (0..3).all(|_| same(parts.next(), folder.next()))
        && matches!(parts.next(), Some(Component::Normal(_)))
        && parts.next().is_none()
}

#[cfg(test)]
#[path = "daemons_tests.rs"]
mod tests;
