//! Backups: every key and value the app removes is saved first, exactly as
//! the registry stored it, and can be put back. A person can also back up
//! any key by hand, or save the whole registry (`full`, not restored here).
//! Each backup is `backups/<id>/backup.json`, beside a full backup's hive
//! files.

mod capture;
mod force;
#[cfg(test)]
#[cfg(windows)]
mod force_tests;
mod full;
#[cfg(test)]
#[cfg(windows)]
mod full_tests;
mod regfile;
mod relocate;
mod remove;
mod restore;
#[cfg(test)]
#[cfg(windows)]
mod tests;
mod trim;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub use capture::{Child, KeyTree, capture_key, write_tree};
pub use full::{FULL_RESTORE, back_up_full};
pub use regfile::{reg_file_bytes, to_reg_file};
pub use relocate::move_backups;
pub use remove::{Removal, Target, describe_target, remove};
pub use restore::{RestoreOutcome, restore};
pub use trim::trim;

use crate::model::{RawValue, RegPath};
use crate::scan::Category;
use crate::store::{read_json, write_json};
use crate::{Error, Result};

pub const MANIFEST: &str = "backup.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupKind {
    /// Taken by the registry cleaner before removing what it found.
    Clean,
    /// Taken before removing registry search matches.
    Search,
    /// A key the person backed up by hand; nothing was removed.
    Manual,
    /// The whole registry's hives, saved as files.
    Full,
}

/// What became of one backed-up entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ItemState {
    /// Saved; the removal had not finished when the manifest was written.
    Pending,
    Removed,
    /// Backed up by hand; still in the registry.
    Kept,
    /// Not removed, and nothing to restore.
    Failed {
        reason: String,
    },
    /// Put back from this backup.
    Restored,
}

/// The saved copy of a whole key or of one value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "data")]
pub enum Saved {
    Key(KeyTree),
    Value(RawValue),
    /// A whole hive: the name of its file beside the manifest.
    Hive(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub key: RegPath,
    pub value: Option<String>,
    pub category: Option<Category>,
    pub detail: String,
    /// `None` when the entry could not be read, so it was not removed.
    pub saved: Option<Saved>,
    /// For a value removed by taking ownership: its key's owner and
    /// permissions (SDDL), put back on the key straight after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<String>,
    #[serde(flatten)]
    pub state: ItemState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub version: u32,
    pub id: String,
    /// Seconds since the Unix epoch.
    pub created: u64,
    pub kind: BackupKind,
    pub label: String,
    pub items: Vec<Item>,
}

/// One line of the backup list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub created: u64,
    pub kind: BackupKind,
    pub label: String,
    pub items: usize,
    pub removed: usize,
    pub restored: usize,
    pub failed: usize,
    /// A backup whose manifest could not be read: shown, never restored.
    pub damaged: Option<String>,
}

impl Backup {
    pub fn summary(&self) -> Summary {
        let count = |wanted: fn(&ItemState) -> bool| {
            self.items.iter().filter(|item| wanted(&item.state)).count()
        };
        Summary {
            id: self.id.clone(),
            created: self.created,
            kind: self.kind,
            label: self.label.clone(),
            items: self.items.len(),
            removed: count(|state| matches!(state, ItemState::Removed | ItemState::Pending)),
            restored: count(|state| matches!(state, ItemState::Restored)),
            failed: count(|state| matches!(state, ItemState::Failed { .. })),
            damaged: None,
        }
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        write_json(&folder(dir, &self.id)?.join(MANIFEST), self)
    }
}

/// A backup id from the page: digits and dashes only, so it can never name
/// a path outside the backups folder.
pub fn folder(dir: &Path, id: &str) -> Result<PathBuf> {
    let valid = !id.is_empty()
        && id.len() <= 40
        && id.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-');
    if !valid {
        return Err(Error::Invalid(format!("\"{id}\" is not a backup")));
    }
    Ok(dir.join(id))
}

pub fn load(dir: &Path, id: &str) -> Result<Backup> {
    read_json(&folder(dir, id)?.join(MANIFEST))?
        .ok_or_else(|| Error::Invalid(format!("Backup {id} no longer exists.")))
}

/// Every backup, newest first. A damaged manifest is listed, not hidden.
pub fn list(dir: &Path) -> Result<Vec<Summary>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::file("list", dir, source)),
    };
    let mut summaries = Vec::new();
    for entry in entries.flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        if folder(dir, &id).is_err() || !entry.path().join(MANIFEST).is_file() {
            continue;
        }
        match load(dir, &id) {
            Ok(backup) => summaries.push(backup.summary()),
            Err(error) => summaries.push(Summary {
                created: id
                    .split('-')
                    .next()
                    .and_then(|seconds| seconds.parse().ok())
                    .unwrap_or(0),
                id,
                kind: BackupKind::Manual,
                label: "Damaged backup".into(),
                items: 0,
                removed: 0,
                restored: 0,
                failed: 0,
                damaged: Some(error.to_string()),
            }),
        }
    }
    summaries.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    Ok(summaries)
}

pub fn delete(dir: &Path, id: &str) -> Result<()> {
    let path = folder(dir, id)?;
    fs::remove_dir_all(&path).map_err(|source| Error::file("delete", path, source))
}

/// Delete cleaner, search and full backups older than `days` days (counted
/// from `now`, in seconds since the Unix epoch); `days` 0 deletes nothing.
/// Full backups are included because each is hundreds of megabytes. Hand
/// backups and damaged ones are left. Returns how many were deleted.
pub fn prune(dir: &Path, days: u32, now: u64) -> Result<usize> {
    if days == 0 {
        return Ok(0);
    }
    let oldest = now.saturating_sub(u64::from(days) * 86_400);
    let mut deleted = 0;
    for summary in list(dir)? {
        if trim::automatic(&summary) && summary.created < oldest {
            delete(dir, &summary.id)?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// A new, unused backup id: the time, and a counter when two share a second.
pub(crate) fn new_id(dir: &Path) -> (String, u64) {
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let mut id = created.to_string();
    let mut counter = 1;
    while dir.join(&id).exists() {
        counter += 1;
        id = format!("{created}-{counter}");
    }
    (id, created)
}

/// Back up a key by hand: nothing is removed.
pub fn back_up_key(
    system: &dyn crate::System,
    dir: &Path,
    key: &RegPath,
    label: &str,
) -> Result<Summary> {
    let tree = capture_key(system, key)?;
    let (id, created) = new_id(dir);
    let backup = Backup {
        version: 1,
        id,
        created,
        kind: BackupKind::Manual,
        label: if label.trim().is_empty() {
            key.display()
        } else {
            label.trim().chars().take(200).collect()
        },
        items: vec![Item {
            key: key.clone(),
            value: None,
            category: None,
            detail: "Backed up by hand.".into(),
            saved: Some(Saved::Key(tree)),
            security: None,
            state: ItemState::Kept,
        }],
    };
    backup.save(dir)?;
    Ok(backup.summary())
}
