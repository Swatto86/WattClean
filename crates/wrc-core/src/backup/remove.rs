//! Backed-up removal: save everything first, then delete, recording what
//! happened to each entry as it goes.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::capture::capture_key;
use super::{Backup, BackupKind, Item, ItemState, Saved, force, new_id};
use crate::Result;
use crate::model::{RegPath, find_value};
use crate::scan::Category;
use crate::system::System;

/// The manifest is saved again after this many removals, so a crash loses
/// little of the record.
const SAVE_EVERY: usize = 50;

/// One thing to remove: a whole key or one of its values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub key: RegPath,
    pub value: Option<String>,
    pub category: Option<Category>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Removal {
    /// `None` when nothing was removed, so no backup was kept.
    pub backup: Option<String>,
    pub removed: usize,
    /// How many of those needed the app to take ownership first.
    pub forced: usize,
    /// Entries removed with something to know, with what.
    pub warnings: Vec<(String, String)>,
    /// Entries that stayed, with the reason.
    pub failed: Vec<(String, String)>,
}

/// Back up every target, save the backup, then remove them. A target that
/// cannot be backed up is not removed. A target inside another target key
/// goes with that key and is not listed twice. With `force`, an entry
/// Windows protects is removed by taking ownership of it (see `force`).
pub fn remove(
    system: &dyn System,
    dir: &Path,
    kind: BackupKind,
    label: &str,
    targets: Vec<Target>,
    force: bool,
) -> Result<Removal> {
    let whole_keys: Vec<RegPath> = targets
        .iter()
        .filter(|target| target.value.is_none())
        .map(|target| target.key.clone())
        .collect();
    let targets: Vec<Target> = targets
        .into_iter()
        .filter(|target| {
            !whole_keys.iter().any(|whole| {
                target.key.is_within(whole)
                    && (target.value.is_some() || target.key.folded() != whole.folded())
            })
        })
        .collect();
    let (id, created) = new_id(dir);
    let mut backup = Backup {
        version: 1,
        id,
        created,
        kind,
        label: label.to_owned(),
        items: targets
            .into_iter()
            .map(|target| save(system, target))
            .collect(),
    };
    backup.save(dir)?;
    let mut done = 0;
    let mut forced = 0;
    let mut warnings = Vec::new();
    for index in 0..backup.items.len() {
        if backup.items[index].saved.is_none() {
            continue;
        }
        let state = match delete(system, &backup.items[index]) {
            Ok(()) => ItemState::Removed,
            Err(error) if force && error.kind() == io::ErrorKind::PermissionDenied => {
                match force::record(system, &mut backup.items[index]) {
                    Err(reason) => ItemState::Failed { reason },
                    Ok(()) => {
                        // The original permissions are on disk before any change.
                        backup.save(dir)?;
                        match force::remove(system, &backup.items[index]) {
                            Ok(warning) => {
                                forced += 1;
                                if let Some(warning) = warning {
                                    warnings.push((describe(&backup.items[index]), warning));
                                }
                                ItemState::Removed
                            }
                            Err(reason) => ItemState::Failed { reason },
                        }
                    }
                }
            }
            Err(error) => ItemState::Failed {
                reason: reason(&error),
            },
        };
        backup.items[index].state = state;
        done += 1;
        if done % SAVE_EVERY == 0 {
            backup.save(dir)?;
        }
    }
    let failed: Vec<(String, String)> = backup
        .items
        .iter()
        .filter_map(|item| match &item.state {
            ItemState::Failed { reason } => Some((describe(item), reason.clone())),
            _ => None,
        })
        .collect();
    let removed = backup
        .items
        .iter()
        .filter(|item| item.state == ItemState::Removed)
        .count();
    let kept = if removed == 0 {
        super::delete(dir, &backup.id)?;
        None
    } else {
        backup.save(dir)?;
        Some(backup.id)
    };
    // Other accounts' hives were opened for the removal; close them now.
    let _ = system.release_account_hives();
    Ok(Removal {
        backup: kept,
        removed,
        forced,
        warnings,
        failed,
    })
}

fn delete(system: &dyn System, item: &Item) -> io::Result<()> {
    match &item.value {
        None => system.delete_key_tree(&item.key),
        Some(name) => system.delete_value(&item.key, name),
    }
}

fn save(system: &dyn System, target: Target) -> Item {
    let saved = match &target.value {
        None => capture_key(system, &target.key)
            .map(Saved::Key)
            .map_err(|error| error.to_string()),
        Some(name) => system
            .values(&target.key)
            .map_err(|error| reason(&error))
            .and_then(|values| {
                find_value(&values, name)
                    .cloned()
                    .map(Saved::Value)
                    .ok_or_else(|| "it is no longer there".to_owned())
            }),
    };
    let (saved, state) = match saved {
        Ok(saved) => (Some(saved), ItemState::Pending),
        Err(reason) => (
            None,
            ItemState::Failed {
                reason: format!("could not be backed up: {reason}"),
            },
        ),
    };
    Item {
        key: target.key,
        value: target.value,
        category: target.category,
        detail: target.detail,
        saved,
        security: None,
        state,
    }
}

/// The entry as a person reads it: `HKCU\Software\X` or `HKCU\Software\X | Name`.
pub fn describe_target(key: &RegPath, value: Option<&str>) -> String {
    match value {
        None => key.display(),
        Some(name) => format!("{} | {}", key.display(), crate::format::value_name(name)),
    }
}

pub(crate) fn describe(item: &Item) -> String {
    describe_target(&item.key, item.value.as_deref())
}

fn reason(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            "access denied: it needs administrator rights, or Windows protects it (Take ownership removes protected entries)".into()
        }
        std::io::ErrorKind::NotFound => "it is no longer there".into(),
        _ => error.to_string(),
    }
}
