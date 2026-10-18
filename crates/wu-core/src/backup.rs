//! Backed-up removal of leftovers, and putting them back.
//!
//! Each removal gets a folder under `backups/`: `backup.json` describes
//! every item (a key's whole tree, a value's raw bytes, where a file was
//! moved to) and is saved before anything is deleted, then after each item,
//! so an interrupted removal can still be restored.

mod remove;
mod restore;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::RawValue;
use crate::scan::Target;
use crate::store::{read_json, write_json};
use crate::system::System;
use crate::{Error, Result};

pub use remove::{Removal, remove};
pub use restore::{RestoreReport, restore};

pub const MANIFEST: &str = "backup.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyTree {
    pub name: String,
    pub values: Vec<RawValue>,
    pub children: Vec<KeyTree>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Saved {
    Key {
        tree: KeyTree,
    },
    Value {
        value: RawValue,
    },
    /// Relative to the backup folder.
    Path {
        stored: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ItemState {
    Pending,
    Removed,
    /// Windows deletes it at the next restart; a copy was kept if `copied`.
    AtRestart {
        copied: bool,
    },
    Failed {
        error: String,
    },
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub target: Target,
    pub saved: Option<Saved>,
    pub state: ItemState,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSet {
    pub version: u32,
    pub id: String,
    pub created: u64,
    pub program: String,
    pub items: Vec<Item>,
    pub restored: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub created: u64,
    pub program: String,
    pub items: usize,
    pub bytes: u64,
    pub restored: Option<u64>,
    pub restorable: bool,
}

impl BackupSet {
    fn summary(&self) -> Summary {
        // What the backup holds now, and what it held before any restore.
        let kept = |item: &&Item| match item.state {
            ItemState::Removed => true,
            ItemState::AtRestart { copied } => copied,
            _ => false,
        };
        let held = |item: &&Item| kept(item) || item.state == ItemState::Restored;
        Summary {
            id: self.id.clone(),
            created: self.created,
            program: self.program.clone(),
            items: self.items.iter().filter(held).count(),
            bytes: self.items.iter().filter(held).map(|item| item.bytes).sum(),
            restored: self.restored,
            restorable: self.restored.is_none() && self.items.iter().any(|item| kept(&item)),
        }
    }

    fn save(&self, dir: &Path) -> Result<()> {
        write_json(&dir.join(MANIFEST), self)
    }
}

/// A backup id names a folder directly under the backups folder; anything
/// else (separators, `..`) is refused before it reaches the file system.
pub fn backup_dir(backups: &Path, id: &str) -> Result<PathBuf> {
    let valid = !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(backups.join(id))
    } else {
        Err(Error::Invalid(format!("{id:?} is not a backup id")))
    }
}

pub fn load(backups: &Path, id: &str) -> Result<BackupSet> {
    let dir = backup_dir(backups, id)?;
    read_json(&dir.join(MANIFEST))?
        .ok_or_else(|| Error::Invalid(format!("backup {id} does not exist")))
}

/// Newest first. A damaged backup is skipped rather than hiding the rest.
pub fn list(backups: &Path) -> Vec<Summary> {
    let mut summaries: Vec<Summary> = std::fs::read_dir(backups)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            load(backups, &name).ok().map(|set| set.summary())
        })
        .collect();
    summaries.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    summaries
}

pub fn delete(backups: &Path, id: &str) -> Result<()> {
    let dir = backup_dir(backups, id)?;
    load(backups, id)?;
    std::fs::remove_dir_all(&dir).map_err(|source| Error::file("delete", dir, source))
}

/// Read a key and everything below it.
pub fn export_tree(system: &dyn System, key: &crate::RegPath) -> std::io::Result<KeyTree> {
    let values = system.values(key)?;
    let mut children = Vec::new();
    for name in system.subkeys(key)? {
        children.push(export_tree(system, &key.child(&name))?);
    }
    Ok(KeyTree {
        name: key.name().to_owned(),
        values,
        children,
    })
}
