//! Putting a removal back, from its backup folder.

use std::fs;
use std::io;
use std::path::Path;

use super::{BackupSet, ItemState, KeyTree, Saved, backup_dir, load};
use crate::model::RegPath;
use crate::scan::Target;
use crate::system::System;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub restored: u32,
    /// The item and why it could not be put back.
    pub failed: Vec<(String, String)>,
}

/// Restore every item the removal took, in reverse order. Nothing that now
/// exists at an original location is overwritten.
pub fn restore(system: &dyn System, backups: &Path, id: &str, now: u64) -> Result<RestoreReport> {
    let dir = backup_dir(backups, id)?;
    let mut set: BackupSet = load(backups, id)?;
    if set.restored.is_some() {
        return Err(Error::Invalid("this backup was already restored".into()));
    }
    let mut report = RestoreReport {
        restored: 0,
        failed: Vec::new(),
    };
    for index in (0..set.items.len()).rev() {
        let item = &set.items[index];
        let restorable = match item.state {
            ItemState::Removed => true,
            ItemState::AtRestart { copied } => copied,
            _ => false,
        };
        let Some(saved) = item.saved.as_ref().filter(|_| restorable) else {
            continue;
        };
        match put_back(system, &dir, &item.target, saved) {
            Ok(()) => {
                set.items[index].state = ItemState::Restored;
                report.restored += 1;
            }
            Err(error) => report
                .failed
                .push((item.target.display(), error.to_string())),
        }
        set.save(&dir)?;
    }
    set.restored = Some(now);
    set.save(&dir)?;
    Ok(report)
}

fn put_back(system: &dyn System, dir: &Path, target: &Target, saved: &Saved) -> io::Result<()> {
    match (target, saved) {
        (Target::Key { key }, Saved::Key { tree }) => write_tree(system, key, tree),
        (Target::Value { key, .. }, Saved::Value { value }) => system.write_value(key, value),
        (Target::File { path } | Target::Folder { path }, Saved::Path { stored }) => {
            let stored = dir.join(stored);
            if path.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "something already exists at the original location",
                ));
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            match fs::rename(&stored, path) {
                Err(error)
                    if error.raw_os_error() == Some(17)
                        || error.kind() == io::ErrorKind::CrossesDevices =>
                {
                    if stored.is_dir() {
                        super::remove::copy_tree(&stored, path)?;
                        fs::remove_dir_all(&stored)
                    } else {
                        fs::copy(&stored, path)?;
                        fs::remove_file(&stored)
                    }
                }
                other => other,
            }
        }
        _ => Err(io::Error::other("the backup does not match the item")),
    }
}

fn write_tree(system: &dyn System, key: &RegPath, tree: &KeyTree) -> io::Result<()> {
    system.create_key(key)?;
    for value in &tree.values {
        system.write_value(key, value)?;
    }
    for child in &tree.children {
        write_tree(system, &key.child(&child.name), child)?;
    }
    Ok(())
}
