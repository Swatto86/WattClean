//! The background jobs a removed app leaves in the account's own
//! `~/Library/LaunchAgents`: launchd starts them at every login, so they
//! keep running (or keep trying to start a program that is gone) after the
//! app. A plist is the app's when it names the app (Apple's
//! `AssociatedBundleIdentifiers`), when the program it starts lies inside
//! the removed bundle, or when its file name or label is one of the app's
//! own names. Plists in `/Library` are judged in `daemons`, never by name,
//! except a Homebrew formula's service that ran for every account, found
//! here by its exact name.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::leftovers::{Leftover, leftover, owns};
use super::remove::safe_name;
use super::{Manager, Package, PackageSystem, daemons, within};

/// WattClean's own identifier (`tauri.conf.json`): a job of its own is
/// never offered or stopped.
const OWN: &str = "co.swatto.wattclean";

/// Whether a job's label or plist name is WattClean's own.
pub(super) fn is_own(name: &str) -> bool {
    name.to_lowercase().starts_with(OWN)
}

/// The label Homebrew gives a formula's background service.
pub(super) fn service_label(id: &str) -> String {
    format!("homebrew.mxcl.{id}")
}

/// Where launchd runs a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Domain {
    /// This account's session (`gui/<uid>`): the plists in
    /// `~/Library/LaunchAgents` and `/Library/LaunchAgents`.
    Session,
    /// The whole machine's (`system`), as root: `/Library/LaunchDaemons`.
    /// Stopping one takes administrator rights.
    System,
}

/// A background job launchd runs, stopped before the file that starts it
/// goes to the Trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub label: String,
    pub domain: Domain,
}

impl Job {
    pub fn new(label: &str, domain: Domain) -> Self {
        Self {
            label: label.to_string(),
            domain,
        }
    }
}

/// What a launchd job's plist says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchdJob {
    pub label: String,
    /// `Program`, else the first of `ProgramArguments`.
    pub program: Option<String>,
    /// `AssociatedBundleIdentifiers`: the apps it belongs to.
    pub associated: Vec<String>,
    /// Every text in the plist, keys and values: what it refers to.
    pub mentions: Vec<String>,
}

impl LaunchdJob {
    /// A plist as `plutil -convert json` prints it; `None` without a label,
    /// which launchd will not load.
    pub fn from_json(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        let label = value.get("Label")?.as_str()?.trim().to_string();
        if label.is_empty() {
            return None;
        }
        let program = value
            .get("Program")
            .and_then(Value::as_str)
            .or_else(|| value.get("ProgramArguments")?.as_array()?.first()?.as_str())
            .map(str::to_string);
        // One identifier may be written as a string rather than a list.
        let associated = match value.get("AssociatedBundleIdentifiers") {
            Some(Value::String(id)) => vec![id.clone()],
            Some(Value::Array(ids)) => ids
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        };
        let mut mentions = Vec::new();
        texts(&value, &mut mentions);
        Some(Self {
            label,
            program,
            associated,
            mentions,
        })
    }
}

/// Every string in a plist's JSON, keys included.
fn texts(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| texts(item, out)),
        Value::Object(entries) => {
            for (key, item) in entries {
                out.push(key.clone());
                texts(item, out);
            }
        }
        _ => {}
    }
}

/// `removed`'s background jobs on macOS: its agents (see [`agents`]), the
/// jobs and helpers it left for every account (`daemons`) and a formula's
/// service that ran for every account.
pub(super) fn leftovers(
    system: &dyn PackageSystem,
    home: &Path,
    removed: &Package,
    mine: &[String],
    theirs: &[String],
    bundle: Option<&str>,
    other_bundles: &[String],
) -> Vec<Leftover> {
    if !system.is_macos() {
        return Vec::new();
    }
    let mut out = agents(system, home, removed, mine, theirs, bundle, other_bundles);
    out.extend(daemons::for_app(system, home, removed, other_bundles));
    out.extend(homebrew_daemon(system, removed));
    out
}

/// The plists in `~/Library/LaunchAgents` of `removed`'s background jobs,
/// each with its label. `mine` are the names it goes by that no other
/// installed program does, `theirs` the names the others go by, `bundle`
/// its bundle identifier and `other_bundles` the others', all in lower
/// case. A plist that names the app or starts a program inside its bundle
/// is the app's; one found only by a name (its file's or its label) stays
/// when its other name is another program's.
fn agents(
    system: &dyn PackageSystem,
    home: &Path,
    removed: &Package,
    mine: &[String],
    theirs: &[String],
    bundle: Option<&str>,
    other_bundles: &[String],
) -> Vec<Leftover> {
    let Ok(entries) = std::fs::read_dir(home.join("Library").join("LaunchAgents")) else {
        return Vec::new();
    };
    let service = (removed.manager == Manager::Homebrew && safe_name(&removed.id))
        .then(|| service_label(&removed.id).to_lowercase());
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let file = entry.file_name().to_string_lossy().to_lowercase();
        let Some(stem) = file.strip_suffix(".plist") else {
            continue;
        };
        let path = entry.path();
        let job = system.launchd_job(&path);
        let label = job.as_ref().map(|job| job.label.to_lowercase());
        if is_own(stem) || label.as_deref().is_some_and(is_own) {
            continue;
        }
        let says = job.as_ref().is_some_and(|job| {
            let names_app = removed.bundle_id.as_deref().is_some_and(|id| {
                job.associated
                    .iter()
                    .any(|named| named.eq_ignore_ascii_case(id))
            });
            let starts_app = removed
                .bundle()
                .zip(job.program.as_deref())
                .is_some_and(|(bundle, program)| within(program, bundle));
            names_app || starts_app
        });
        let names: Vec<&str> = std::iter::once(stem).chain(label.as_deref()).collect();
        let by_name = names.iter().any(|name| {
            mine.iter().any(|key| key == name)
                || bundle.is_some_and(|id| owns(id, name, other_bundles))
                || service.as_deref() == Some(*name)
        });
        let anothers = names
            .iter()
            .any(|name| theirs.iter().any(|key| key == name));
        if says || (by_name && !anothers) {
            let mut found = leftover(&path);
            found.job = job.map(|job| Job::new(&job.label, Domain::Session));
            out.push(found);
        }
    }
    out
}

/// Gives each plist in the account's LaunchAgents found another way (a
/// cask's zap list) the label of the job it starts, so that job too is
/// stopped before the plist goes.
pub(super) fn label_agents(system: &dyn PackageSystem, home: &Path, out: &mut [Leftover]) {
    if !system.is_macos() {
        return;
    }
    let agents = home.join("Library").join("LaunchAgents");
    for found in out
        .iter_mut()
        .filter(|found| found.job.is_none() && found.path.parent() == Some(&agents))
    {
        found.job = system
            .launchd_job(&found.path)
            .map(|job| Job::new(&job.label, Domain::Session));
    }
}

/// The plist of a formula's service that ran for every account
/// (`/Library/LaunchDaemons`), by its exact name: stopped before the
/// removal, and left behind by Homebrew.
fn homebrew_daemon(system: &dyn PackageSystem, removed: &Package) -> Option<Leftover> {
    if removed.manager != Manager::Homebrew || !safe_name(&removed.id) {
        return None;
    }
    let plist = format!(
        "/Library/LaunchDaemons/{}.plist",
        service_label(&removed.id)
    );
    let path = system.at(&plist);
    std::fs::symlink_metadata(&path).is_ok().then(|| Leftover {
        system_wide: true,
        ..leftover(&path)
    })
}

#[cfg(test)]
#[path = "launchd_tests.rs"]
mod tests;
