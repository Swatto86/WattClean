//! Install logs: what a traced install left on the machine, kept so the
//! program's uninstall can remove exactly that.
//!
//! A trace lists changes in order. The log keeps what still stands at the
//! end: things the installer created (offered for removal later) and things
//! it changed that existed before (shown, never removed). Anything created
//! and removed again during the install, and Windows' own background
//! activity, is dropped.

mod noise;
#[cfg(test)]
#[cfg(windows)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::folders::Folders;
use crate::model::RegPath;
use crate::programs::uninstall_roots;
use crate::scan::Target;
use crate::store::{read_json, write_json};
use crate::system::{System, text_value, values_or_empty};
use crate::trace::{Change, Trace};
use crate::{Error, Program, Result};

pub const DIR: &str = "logs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Recorded {
    /// Made by the installer: removed with the program.
    Created,
    /// Existed before and was changed: shown, never removed.
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogItem {
    pub target: Target,
    pub recorded: Recorded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallLog {
    pub version: u32,
    pub id: String,
    pub created: u64,
    pub installer: String,
    /// The program the install registered, read from its uninstall entry.
    pub program: Option<String>,
    pub program_key: Option<RegPath>,
    pub items: Vec<LogItem>,
    pub processes: Vec<String>,
    pub events_lost: u32,
    pub unnamed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSummary {
    pub id: String,
    pub created: u64,
    pub installer: String,
    pub program: Option<String>,
    /// The program's uninstall key, as a program id (`HKLM\…\Uninstall\X`).
    pub program_id: Option<String>,
    pub created_items: usize,
    pub changed_items: usize,
    pub events_lost: u32,
}

impl InstallLog {
    pub fn summary(&self) -> LogSummary {
        let created = self
            .items
            .iter()
            .filter(|item| item.recorded == Recorded::Created)
            .count();
        LogSummary {
            id: self.id.clone(),
            created: self.created,
            installer: self.installer.clone(),
            program: self.program.clone(),
            program_id: self.program_key.as_ref().map(RegPath::display),
            created_items: created,
            changed_items: self.items.len() - created,
            events_lost: self.events_lost,
        }
    }

    /// Folders the installer created directly in a program root, which make
    /// a program's install folders when its entry names none.
    pub fn install_dirs(&self, folders: &Folders) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter(|item| item.recorded == Recorded::Created)
            .filter_map(|item| match &item.target {
                Target::Folder { path } => Some(path),
                _ => None,
            })
            .filter(|path| {
                folders.program_roots().iter().any(|root| {
                    path.parent()
                        .is_some_and(|parent| crate::paths::same(parent, root))
                })
            })
            .cloned()
            .collect()
    }
}

/// Fold a trace into a log of what the install left behind.
pub fn build(
    system: &dyn System,
    folders: &Folders,
    trace: &Trace,
    installer: &str,
    now: u64,
) -> InstallLog {
    let mut state = State::default();
    for change in &trace.changes {
        state.apply(change);
    }
    let mut items: Vec<LogItem> = state
        .items
        .into_iter()
        .flatten()
        .filter(|item| !noise::is_noise(&item.target, folders))
        .collect();
    let kept = items.clone();
    items.retain(|item| {
        !kept.iter().any(|outer| {
            outer.recorded == Recorded::Created && crate::scan::covers(&outer.target, &item.target)
        })
    });
    items.sort_by_cached_key(|item| {
        (
            !item.target.is_registry(),
            item.target.display().to_lowercase(),
        )
    });
    let program_key = registered_program(&items);
    let program = program_key
        .as_ref()
        .and_then(|key| text_value(&values_or_empty(system, key), "DisplayName"));
    let slug: String = program
        .as_deref()
        .unwrap_or(installer)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(32)
        .collect::<String>()
        .to_lowercase();
    InstallLog {
        version: 1,
        id: format!("{now}-{slug}"),
        created: now,
        installer: installer.to_owned(),
        program,
        program_key,
        items,
        processes: trace.processes.clone(),
        events_lost: trace.events_lost,
        unnamed: trace.unnamed,
    }
}

/// The uninstall entry the install wrote, preferring one it created.
fn registered_program(items: &[LogItem]) -> Option<RegPath> {
    let roots: Vec<RegPath> = uninstall_roots()
        .into_iter()
        .map(|(root, _)| root)
        .collect();
    let entry = |key: &RegPath| {
        let parent = key.parent()?;
        roots
            .iter()
            .any(|root| root.folded() == parent.folded())
            .then(|| key.clone())
    };
    let keys = items.iter().filter_map(|item| match &item.target {
        Target::Key { key } => entry(key).map(|key| (item.recorded, key)),
        Target::Value { key, .. } => entry(key).map(|key| (item.recorded, key)),
        _ => None,
    });
    let mut found: Vec<(Recorded, RegPath)> = keys.collect();
    found.sort_by_key(|(recorded, _)| *recorded != Recorded::Created);
    found.into_iter().next().map(|(_, key)| key)
}

/// The log being built: items in first-seen order, removable when undone.
#[derive(Default)]
struct State {
    items: Vec<Option<LogItem>>,
    index: HashMap<String, usize>,
}

fn identity(target: &Target) -> String {
    match target {
        Target::Key { key } => format!("k|{}", key.folded()),
        Target::Value { key, name } => format!("v|{}|{}", key.folded(), name.to_lowercase()),
        Target::Folder { path } | Target::File { path } => {
            format!("p|{}", crate::paths::fold(path))
        }
    }
}

impl State {
    fn get(&self, target: &Target) -> Option<Recorded> {
        let at = *self.index.get(&identity(target))?;
        self.items.get(at)?.as_ref().map(|item| item.recorded)
    }

    fn put(&mut self, target: Target, recorded: Recorded) {
        let key = identity(&target);
        match self.index.get(&key) {
            Some(&at) => self.items[at] = Some(LogItem { target, recorded }),
            None => {
                self.index.insert(key, self.items.len());
                self.items.push(Some(LogItem { target, recorded }));
            }
        }
    }

    /// Forget `target` and everything inside it (a deleted folder or key).
    fn remove_within(&mut self, target: &Target) {
        for slot in &mut self.items {
            if slot.as_ref().is_some_and(|item| {
                identity(&item.target) == identity(target)
                    || crate::scan::covers(target, &item.target)
            }) {
                *slot = None;
            }
        }
        self.index
            .retain(|_, at| self.items.get(*at).is_some_and(Option::is_some));
    }

    fn key_created(&self, key: &RegPath) -> bool {
        let mut current = Some(key.clone());
        while let Some(key) = current {
            if self.get(&Target::Key { key: key.clone() }) == Some(Recorded::Created) {
                return true;
            }
            current = key.parent();
        }
        false
    }

    fn apply(&mut self, change: &Change) {
        let file = |path: &Path| Target::File {
            path: path.to_path_buf(),
        };
        match change {
            Change::FileCreated { path } => self.put(file(path), Recorded::Created),
            Change::FolderCreated { path } => {
                self.put(Target::Folder { path: path.clone() }, Recorded::Created)
            }
            Change::FileReplaced { path } | Change::FileRenamed { path } => {
                if self.get(&file(path)).is_none() {
                    self.put(file(path), Recorded::Changed);
                }
            }
            Change::FileDeleted { path } => {
                self.remove_within(&Target::Folder { path: path.clone() });
            }
            Change::KeyCreated { key } => {
                self.put(Target::Key { key: key.clone() }, Recorded::Created)
            }
            Change::KeyDeleted { key } => self.remove_within(&Target::Key { key: key.clone() }),
            Change::ValueSet { key, name, existed } => {
                let target = Target::Value {
                    key: key.clone(),
                    name: name.clone(),
                };
                if self.key_created(key) || self.get(&target) == Some(Recorded::Created) {
                    return;
                }
                self.put(
                    target,
                    if *existed {
                        Recorded::Changed
                    } else {
                        Recorded::Created
                    },
                );
            }
            Change::ValueDeleted { key, name } => {
                self.remove_within(&Target::Value {
                    key: key.clone(),
                    name: name.clone(),
                });
            }
        }
    }
}

fn log_path(dir: &Path, id: &str) -> Result<PathBuf> {
    let valid = !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(dir.join(DIR).join(format!("{id}.json")))
    } else {
        Err(Error::Invalid(format!("{id:?} is not an install log id")))
    }
}

/// Save a new log; a second log of the same second gets a numbered id.
pub fn save(dir: &Path, log: &mut InstallLog) -> Result<()> {
    let base = log.id.clone();
    let mut attempt = 1;
    while log_path(dir, &log.id)?.exists() {
        log.id = format!("{base}-{attempt}");
        attempt += 1;
    }
    write_json(&log_path(dir, &log.id)?, log)
}

pub fn load(dir: &Path, id: &str) -> Result<InstallLog> {
    read_json(&log_path(dir, id)?)?
        .ok_or_else(|| Error::Invalid(format!("install log {id} does not exist")))
}

/// Newest first. A damaged log is skipped rather than hiding the rest.
pub fn list(dir: &Path) -> Vec<InstallLog> {
    let mut logs: Vec<InstallLog> = std::fs::read_dir(dir.join(DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".json")?;
            load(dir, id).ok()
        })
        .collect();
    logs.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    logs
}

pub fn delete(dir: &Path, id: &str) -> Result<()> {
    let path = log_path(dir, id)?;
    load(dir, id)?;
    std::fs::remove_file(&path).map_err(|source| Error::file("delete", path, source))
}

/// The newest log of the install that registered `program`.
pub fn for_program(dir: &Path, program: &Program) -> Option<InstallLog> {
    let key = program.registry_key.folded();
    list(dir).into_iter().find(|log| {
        log.program_key
            .as_ref()
            .is_some_and(|logged| logged.folded() == key)
    })
}
