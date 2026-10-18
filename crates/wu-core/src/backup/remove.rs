//! Removing the leftovers the person ticked, each one backed up first.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{BackupSet, Item, ItemState, Saved, backup_dir, export_tree};
use crate::history::Counts;
use crate::model::find_value;
use crate::scan::{Leftover, Target};
use crate::system::System;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub backup: BackupSet,
    pub removed: Counts,
    pub at_restart: u32,
    /// The item and why it could not be removed.
    pub failed: Vec<(String, String)>,
    pub bytes: u64,
}

pub fn remove(
    system: &dyn System,
    backups: &Path,
    program: &str,
    leftovers: &[Leftover],
    now: u64,
) -> Result<Removal> {
    let (id, dir) = new_backup_dir(backups, program, now)?;
    fs::create_dir_all(dir.join("files")).map_err(|source| Error::file("create", &dir, source))?;

    let mut ordered: Vec<&Leftover> = leftovers.iter().collect();
    // Values before keys, files before folders, deepest first, so nothing is
    // removed from under an item still waiting its turn.
    ordered.sort_by_key(|leftover| {
        let (rank, depth) = match &leftover.target {
            Target::Value { key, .. } => (0, key.depth()),
            Target::Key { key } => (1, key.depth()),
            Target::File { path } => (2, path.components().count()),
            Target::Folder { path } => (3, path.components().count()),
        };
        (rank, std::cmp::Reverse(depth))
    });
    let items = ordered
        .iter()
        .enumerate()
        .map(|(index, leftover)| prepare(system, index, leftover))
        .collect();
    let mut set = BackupSet {
        version: 1,
        id,
        created: now,
        program: program.to_owned(),
        items,
        restored: None,
    };
    set.save(&dir)?;

    let mut removal = Removal {
        backup: set.clone(),
        removed: Counts::default(),
        at_restart: 0,
        failed: Vec::new(),
        bytes: 0,
    };
    for index in 0..set.items.len() {
        let state = execute(system, &dir, &set.items[index]);
        let item = &mut set.items[index];
        item.state = state;
        match &item.state {
            ItemState::Removed => {
                count(&mut removal.removed, &item.target);
                removal.bytes += item.bytes;
            }
            ItemState::AtRestart { .. } => {
                removal.at_restart += 1;
                removal.bytes += item.bytes;
            }
            ItemState::Failed { error } => {
                removal.failed.push((item.target.display(), error.clone()))
            }
            ItemState::Pending | ItemState::Restored => {}
        }
        set.save(&dir)?;
    }
    removal.backup = set;
    Ok(removal)
}

fn new_backup_dir(backups: &Path, program: &str, now: u64) -> Result<(String, PathBuf)> {
    let slug: String = program
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(32)
        .collect::<String>()
        .to_lowercase();
    for attempt in 0..1000u32 {
        let id = if attempt == 0 {
            format!("{now}-{slug}")
        } else {
            format!("{now}-{slug}-{attempt}")
        };
        let dir = backup_dir(backups, &id)?;
        if !dir.exists() {
            return Ok((id, dir));
        }
    }
    Err(Error::Invalid(
        "could not find a free backup folder name".into(),
    ))
}

fn prepare(system: &dyn System, index: usize, leftover: &Leftover) -> Item {
    let saved = match &leftover.target {
        Target::Key { key } => export_tree(system, key).map(|tree| Saved::Key { tree }),
        Target::Value { key, name } => system.values(key).and_then(|values| {
            find_value(&values, name)
                .cloned()
                .map(|value| Saved::Value { value })
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "the value no longer exists")
                })
        }),
        Target::File { .. } | Target::Folder { .. } => Ok(Saved::Path {
            stored: format!("files/{index}"),
        }),
    };
    let (saved, state) = match saved {
        Ok(saved) => (Some(saved), ItemState::Pending),
        Err(error) => (
            None,
            ItemState::Failed {
                error: format!("could not back it up: {error}"),
            },
        ),
    };
    Item {
        target: leftover.target.clone(),
        saved,
        state,
        bytes: leftover.bytes,
    }
}

fn execute(system: &dyn System, dir: &Path, item: &Item) -> ItemState {
    if item.state != ItemState::Pending {
        return item.state.clone();
    }
    let result = match (&item.target, &item.saved) {
        (Target::Key { key }, _) => system.delete_key_tree(key).map(|()| ItemState::Removed),
        (Target::Value { key, name }, _) => {
            system.delete_value(key, name).map(|()| ItemState::Removed)
        }
        (Target::File { path } | Target::Folder { path }, Some(Saved::Path { stored })) => {
            move_out(system, path, &dir.join(stored))
        }
        _ => Err(io::Error::other("nothing was backed up for this item")),
    };
    result.unwrap_or_else(|error| ItemState::Failed {
        error: error.to_string(),
    })
}

/// Move a file or folder into the backup. A rename is instant on the same
/// drive; across drives it is copied then deleted. What Windows will not let
/// go of (a file in use) is copied if it can be read, then left for Windows
/// to delete at the next restart.
fn move_out(system: &dyn System, path: &Path, stored: &Path) -> io::Result<ItemState> {
    let metadata = fs::symlink_metadata(path)?;
    match fs::rename(path, stored) {
        Ok(()) => return Ok(ItemState::Removed),
        Err(error) if !is_busy_or_cross_drive(&error) => return Err(error),
        Err(_) => {}
    }
    let copied = if metadata.is_dir() {
        copy_tree(path, stored).is_ok()
    } else {
        fs::copy(path, stored).is_ok()
    };
    let gone = if metadata.is_dir() {
        fs::remove_dir_all(path).is_ok()
    } else {
        fs::remove_file(path).is_ok()
    };
    if gone && copied {
        return Ok(ItemState::Removed);
    }
    schedule_remaining(system, path)?;
    Ok(ItemState::AtRestart { copied })
}

fn is_busy_or_cross_drive(error: &io::Error) -> bool {
    // ERROR_ACCESS_DENIED (5), ERROR_SHARING_VIOLATION (32),
    // ERROR_LOCK_VIOLATION (33), ERROR_NOT_SAME_DEVICE (17).
    matches!(error.raw_os_error(), Some(5 | 17 | 32 | 33))
        || error.kind() == io::ErrorKind::CrossesDevices
}

/// Copy a folder without following links, so a junction inside a program
/// folder never pulls another tree into the backup.
pub(super) fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Ask Windows to delete whatever is still left of `path` at the next restart:
/// files first, then their folders, deepest first.
fn schedule_remaining(system: &dyn System, path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(current) = pending.pop() {
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.is_dir() {
            for entry in fs::read_dir(&current)?.flatten() {
                pending.push(entry.path());
            }
            dirs.push(current);
        } else {
            files.push(current);
        }
    }
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    for target in files.iter().chain(&dirs) {
        system.delete_on_reboot(target)?;
    }
    Ok(())
}

fn count(counts: &mut Counts, target: &Target) {
    match target {
        Target::Key { .. } => counts.keys += 1,
        Target::Value { .. } => counts.values += 1,
        Target::Folder { .. } => counts.folders += 1,
        Target::File { .. } => counts.files += 1,
    }
}
