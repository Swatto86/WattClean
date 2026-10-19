//! Finding what an uninstaller left behind.
//!
//! Three modes, each including the one before: Safe looks where a program
//! keeps its own folders and keys under its exact name; Moderate also follows
//! the program's install path through Windows' shared registrations (App
//! Paths, Run, COM classes, services, firewall rules) and accepts name
//! prefixes; Advanced also walks the software registry and data folders
//! deeply. Every candidate passes the [`guard`] before it is offered.

mod classes;
mod clients;
mod deep;
mod files;
pub mod guard;
mod installer;
mod logged;
mod openwith;
mod registrations;
mod registry;
pub mod shortcut;
mod tasks;
#[cfg(test)]
pub(crate) mod tests;
mod users;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::folders::Folders;
use crate::identity::{Identity, Match};
use crate::model::{Program, RegPath};
use crate::paths::is_within;
use crate::system::System;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Safe,
    Moderate,
    Advanced,
}

impl Mode {
    fn name_match(self) -> Match {
        match self {
            Self::Safe => Match::Exact,
            Self::Moderate => Match::Prefix,
            Self::Advanced => Match::Contains,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    Key { key: RegPath },
    Value { key: RegPath, name: String },
    Folder { path: PathBuf },
    File { path: PathBuf },
}

impl Target {
    pub fn is_registry(&self) -> bool {
        matches!(self, Self::Key { .. } | Self::Value { .. })
    }

    pub fn display(&self) -> String {
        match self {
            Self::Key { key } => key.display(),
            Self::Value { key, name } => format!("{} → {}", key.display(), value_label(name)),
            Self::Folder { path } | Self::File { path } => path.display().to_string(),
        }
    }
}

pub fn value_label(name: &str) -> &str {
    if name.is_empty() { "(Default)" } else { name }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    pub id: u32,
    pub target: Target,
    pub reason: String,
    pub confidence: Confidence,
    /// Ticked when offered; low-confidence finds start unticked.
    pub selected: bool,
    /// Bytes on disk for files and folders.
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub mode: Mode,
    pub items: Vec<Leftover>,
    /// Things the person should know, such as a folder shared with another program.
    pub warnings: Vec<String>,
}

/// Progress reporting and cancellation for a running scan.
pub trait Progress: Sync {
    fn area(&self, area: &str);
    fn cancelled(&self) -> bool;
}

/// Another installed program, which the scan must not damage.
#[derive(Debug, Clone)]
pub struct OtherProgram {
    pub name: String,
    pub key: RegPath,
    pub dirs: Vec<PathBuf>,
    pub names: Vec<String>,
    pub publisher_key: Option<String>,
}

impl OtherProgram {
    /// Every listed program except `target` and other entries of the same
    /// program (the same name, or the same install folder), which are
    /// leftovers of it rather than programs to protect.
    pub fn all(
        system: &dyn System,
        folders: &Folders,
        programs: &[Program],
        target: Option<&Program>,
    ) -> Vec<Self> {
        let target_dirs: Vec<PathBuf> = target
            .map(|program| Identity::of_entry(system, folders, program).install_dirs)
            .unwrap_or_default();
        let same_program = |program: &Program| {
            target.is_some_and(|target| {
                program.id.eq_ignore_ascii_case(&target.id)
                    || crate::identity::name_key(&program.name)
                        == crate::identity::name_key(&target.name)
            })
        };
        programs
            .iter()
            .filter(|program| !same_program(program))
            .filter(|program| {
                let dirs = Identity::of_entry(system, folders, program).install_dirs;
                !dirs
                    .iter()
                    .any(|dir| target_dirs.iter().any(|ours| crate::paths::same(dir, ours)))
            })
            .map(|program| {
                let identity = Identity::of_entry(system, folders, program);
                Self {
                    name: program.name.clone(),
                    key: program.registry_key.clone(),
                    names: identity.all_names(),
                    dirs: identity.install_dirs,
                    publisher_key: identity.publisher_key,
                }
            })
            .collect()
    }
}

pub struct Context<'a> {
    pub system: &'a dyn System,
    pub folders: &'a Folders,
    pub others: &'a [OtherProgram],
    pub progress: &'a dyn Progress,
    /// The program's install log, when it was installed with tracing.
    pub log: Option<&'a crate::install_log::InstallLog>,
    /// Folders never offered whatever matched: the app's own data and program.
    pub protected: &'a [PathBuf],
}

impl Context<'_> {
    /// True when `name` is exactly another installed program's name: the
    /// folder or key is theirs, however well it matches ours.
    fn belongs_to_other(&self, name: &str) -> bool {
        let key = crate::identity::name_key(name);
        self.others.iter().any(|other| other.names.contains(&key))
    }

    fn publisher_shared(&self, identity: &Identity) -> bool {
        identity.publisher_key.as_ref().is_some_and(|own| {
            self.others
                .iter()
                .any(|other| other.publisher_key.as_ref() == Some(own))
        })
    }
}

/// Candidates gathered by the scanners, before ordering and de-duplication.
#[derive(Default)]
struct Found {
    items: Vec<(Target, String, Confidence)>,
    warnings: Vec<String>,
}

impl Found {
    fn add(&mut self, target: Target, reason: impl Into<String>, confidence: Confidence) {
        let reason = reason.into();
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|(known, _, _)| same_target(known, &target))
        {
            if confidence > existing.2 {
                *existing = (target, reason, confidence);
            }
            return;
        }
        self.items.push((target, reason, confidence));
    }

    fn warn(&mut self, warning: String) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    fn contains(&self, target: &Target) -> bool {
        self.items
            .iter()
            .any(|(known, _, _)| same_target(known, target))
    }

    /// Is `key` one of the keys found so far, or inside one?
    fn covers_key(&self, key: &RegPath) -> bool {
        self.items.iter().any(|(known, _, _)| match known {
            Target::Key { key: found } => key.is_within(found),
            _ => false,
        })
    }
}

fn same_target(a: &Target, b: &Target) -> bool {
    match (a, b) {
        (Target::Key { key: x }, Target::Key { key: y }) => x.folded() == y.folded(),
        (Target::Value { key: x, name: m }, Target::Value { key: y, name: n }) => {
            x.folded() == y.folded() && m.eq_ignore_ascii_case(n)
        }
        (Target::Folder { path: x }, Target::Folder { path: y })
        | (Target::File { path: x }, Target::File { path: y }) => crate::paths::same(x, y),
        _ => false,
    }
}

/// Does deleting `outer` also remove `inner`?
pub(crate) fn covers(outer: &Target, inner: &Target) -> bool {
    covered_by(inner, outer)
}

/// Is `inner` already covered by deleting `outer`?
fn covered_by(inner: &Target, outer: &Target) -> bool {
    match (inner, outer) {
        (Target::Key { key } | Target::Value { key, .. }, Target::Key { key: parent }) => {
            key.is_within(parent) && !same_target(inner, outer)
        }
        (Target::Folder { path } | Target::File { path }, Target::Folder { path: parent }) => {
            is_within(path, parent) && !same_target(inner, outer)
        }
        _ => false,
    }
}

/// One scanner: adds what it finds for identity in mode.
type Step = fn(&Identity, Mode, &Context<'_>, &mut Found);

pub fn scan(identity: &Identity, mode: Mode, context: &Context<'_>) -> ScanReport {
    let mut found = Found::default();
    // A program installed inside this one's folder (a plugin pack) keeps
    // its registrations, whatever path they share with ours.
    let scoped = identity.with_others(context.others);
    let identity = &scoped;
    for other in context.others {
        if identity.install_dirs.iter().any(|dir| {
            other
                .dirs
                .iter()
                .any(|theirs| crate::paths::same(dir, theirs))
        }) {
            found.warn(format!(
                "{} is installed in the same folder; that folder is left in place.",
                other.name
            ));
        }
    }
    let steps: [(&str, Step); 11] = [
        ("Install log", logged::scan),
        ("Program folders", files::scan),
        ("Software registry", registry::scan),
        ("Windows Installer records", installer::scan),
        ("Windows registrations", registrations::scan),
        ("Scheduled tasks and event logs", tasks::scan),
        ("Default programs", clients::scan),
        ("COM classes and file types", classes::scan),
        ("Open-with lists", openwith::scan),
        ("Other user accounts", users::scan),
        ("Deep search", deep::scan),
    ];
    for (area, step) in steps {
        if context.progress.cancelled() {
            break;
        }
        context.progress.area(area);
        step(identity, mode, context, &mut found);
    }
    finish(mode, found)
}

fn finish(mode: Mode, found: Found) -> ScanReport {
    let Found { items, warnings } = found;
    let kept: Vec<_> = items
        .iter()
        .filter(|(target, _, _)| !items.iter().any(|(outer, _, _)| covered_by(target, outer)))
        .cloned()
        .collect();
    let mut sorted = kept;
    sorted.sort_by_cached_key(|(target, _, _)| {
        (!target.is_registry(), target.display().to_lowercase())
    });
    let items = sorted
        .into_iter()
        .zip(1u32..)
        .map(|((target, reason, confidence), id)| {
            let bytes = match &target {
                Target::Folder { path } => folder_bytes(path),
                Target::File { path } => {
                    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
                }
                _ => 0,
            };
            Leftover {
                id,
                selected: confidence >= Confidence::Medium,
                target,
                reason,
                confidence,
                bytes,
            }
        })
        .collect();
    ScanReport {
        mode,
        items,
        warnings,
    }
}

/// Total size of the regular files under `path`, not following links.
pub fn folder_bytes(path: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![path.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(entry.path()),
                Ok(kind) if kind.is_file() => {
                    total += entry.metadata().map(|meta| meta.len()).unwrap_or(0)
                }
                _ => {}
            }
        }
    }
    total
}
