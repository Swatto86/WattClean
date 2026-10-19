//! Putting a backup back. Nothing at the original place is ever overwritten:
//! an entry that has come back since is left as it is and reported.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::capture::write_tree;
use super::remove::{describe, partly_removed};
use super::{BackupKind, FULL_RESTORE, Item, ItemState, Saved, force, load};
use crate::model::find_value;
use crate::system::{System, key_exists};
use crate::{Error, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub restored: usize,
    /// Entries not put back, with the reason.
    pub skipped: Vec<(String, String)>,
}

/// Restore the chosen items of a backup (by position), or all of them. The
/// backup records what was restored even when a later item fails.
pub fn restore(
    system: &dyn System,
    dir: &Path,
    id: &str,
    chosen: Option<&[usize]>,
) -> Result<RestoreOutcome> {
    let mut backup = load(dir, id)?;
    if let Some(chosen) = chosen
        && let Some(bad) = chosen.iter().find(|index| **index >= backup.items.len())
    {
        return Err(Error::Invalid(format!("Backup {id} has no item {bad}.")));
    }
    if backup.kind == BackupKind::Full {
        return Err(Error::Invalid(FULL_RESTORE.into()));
    }
    let mut outcome = RestoreOutcome::default();
    let mut failure = None;
    for (index, item) in backup.items.iter_mut().enumerate() {
        if chosen.is_some_and(|chosen| !chosen.contains(&index)) {
            continue;
        }
        if let Err(error) = restore_item(system, item, &mut outcome) {
            failure = Some(error);
            break;
        }
    }
    backup.save(dir)?;
    let _ = system.release_account_hives();
    match failure {
        Some(error) => Err(error),
        None => Ok(outcome),
    }
}

fn restore_item(system: &dyn System, item: &mut Item, outcome: &mut RestoreOutcome) -> Result<()> {
    let what = describe(item);
    let mut skip = |why: &str| outcome.skipped.push((what.clone(), why.to_owned()));
    let Some(saved) = &item.saved else {
        skip("it was never removed");
        return Ok(());
    };
    match (&item.state, saved) {
        (ItemState::Restored, _) => skip("it was already restored"),
        (_, Saved::Hive(_)) => skip(FULL_RESTORE),
        // A key delete stopped part-way: put back what went, keep the rest.
        (ItemState::Failed { .. }, Saved::Key(tree)) if partly_removed(system, item) => {
            let left = write_tree(system, &item.key, tree, true)?;
            if left > 0 {
                skip(&format!("{left} values still there were left as they are"));
            }
            item.state = ItemState::Restored;
            outcome.restored += 1;
        }
        (ItemState::Failed { .. }, _) => skip("it was never removed"),
        (ItemState::Kept, Saved::Key(tree)) => {
            let left = write_tree(system, &item.key, tree, true)?;
            if left > 0 {
                skip(&format!(
                    "{left} values already there were left as they are"
                ));
            }
            outcome.restored += 1;
        }
        (_, Saved::Key(tree)) => {
            if key_exists(system, &item.key) {
                skip("the key is there again, so it was left as it is");
                return Ok(());
            }
            if force::has_security(tree) {
                // It was protected: its parent may be too, so create it as
                // its owner would, then protect it again as it was.
                if let Some(parent) = item.key.parent() {
                    force::with_owned(system, &parent, || system.create_key(&item.key))
                        .map_err(|source| Error::registry("create", &item.key, source))?;
                }
                write_tree(system, &item.key, tree, false)?;
                if let Err(error) = force::apply_security(system, &item.key, tree) {
                    skip(&format!(
                        "it was restored, but its original permissions could not be put back: {error}"
                    ));
                }
            } else {
                write_tree(system, &item.key, tree, false)?;
            }
            item.state = ItemState::Restored;
            outcome.restored += 1;
        }
        (_, Saved::Value(value)) => {
            let present = system
                .values(&item.key)
                .is_ok_and(|values| find_value(&values, &value.name).is_some());
            if present {
                skip("a value of that name is there again, so it was left as it is");
                return Ok(());
            }
            let written = if item.security.is_some() {
                force::with_owned(system, &item.key, || system.write_value(&item.key, value))
            } else {
                system.write_value(&item.key, value)
            };
            written.map_err(|source| Error::registry("write", &item.key, source))?;
            item.state = ItemState::Restored;
            outcome.restored += 1;
        }
    }
    Ok(())
}
