//! Putting a removal back, from its backup folder.

use std::fs;
use std::io;
use std::path::Path;

use super::{BackupSet, ItemState, KeyTree, Saved, backup_dir, export_tree, load};
use crate::model::{RegPath, find_value};
use crate::scan::Target;
use crate::system::{System, key_exists, values_or_empty};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub restored: u32,
    /// The item and why it could not be put back.
    pub failed: Vec<(String, String)>,
}

/// Restore every item the removal took, in reverse order. Nothing that now
/// exists at an original location is overwritten. Items that could not be put
/// back stay restorable, so a later call retries just those.
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
        let Some(saved) = item.saved.as_ref() else {
            continue;
        };
        let restorable = match item.state {
            ItemState::Removed => true,
            ItemState::AtRestart { copied } => copied,
            // The app died between deleting it and saving that it had: the
            // data is in the backup, so put it back if the original is gone.
            ItemState::Pending => original_gone(system, &dir, &item.target, saved),
            _ => false,
        };
        if !restorable {
            continue;
        }
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
    // Only a complete restore closes the backup.
    if report.failed.is_empty() {
        set.restored = Some(now);
    }
    set.save(&dir)?;
    Ok(report)
}

fn original_gone(system: &dyn System, dir: &Path, target: &Target, saved: &Saved) -> bool {
    match (target, saved) {
        (Target::Key { key }, _) => !key_exists(system, key),
        (Target::Value { key, name }, _) => {
            find_value(&values_or_empty(system, key), name).is_none()
        }
        (Target::File { path } | Target::Folder { path }, Saved::Path { stored }) => {
            fs::symlink_metadata(path).is_err() && dir.join(stored).exists()
        }
        _ => false,
    }
}

fn put_back(system: &dyn System, dir: &Path, target: &Target, saved: &Saved) -> io::Result<()> {
    match (target, saved) {
        (Target::Key { key }, Saved::Key { tree }) => {
            if !key_exists(system, key) {
                return write_tree(system, key, tree);
            }
            // Back already (a parent's restore, or the program returned).
            if same_tree(&export_tree(system, key)?, tree) {
                return Ok(());
            }
            Err(exists("the key exists again with different contents"))
        }
        (Target::Value { key, .. }, Saved::Value { value }) => {
            match find_value(&values_or_empty(system, key), &value.name) {
                None => system.write_value(key, value),
                Some(now) if now == value => Ok(()),
                Some(_) => Err(exists("the value exists again with other data")),
            }
        }
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

fn exists(why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("already exists: {why}"),
    )
}

fn same_tree(a: &KeyTree, b: &KeyTree) -> bool {
    a.values.len() == b.values.len()
        && b.values
            .iter()
            .all(|value| find_value(&a.values, &value.name) == Some(value))
        && a.children.len() == b.children.len()
        && b.children.iter().all(|child| {
            a.children
                .iter()
                .find(|other| other.name.eq_ignore_ascii_case(&child.name))
                .is_some_and(|other| same_tree(other, child))
        })
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
