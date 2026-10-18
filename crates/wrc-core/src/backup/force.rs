//! Removing what Windows protects. The app takes ownership of the key and
//! gives itself full control, but only after the key's original owner and
//! permissions are recorded in the backup, so restoring puts the protection
//! back. A key that stays (one value removed) gets its permissions back at
//! once.

use std::io;

use super::capture::KeyTree;
use super::{Item, Saved};
use crate::model::RegPath;
use crate::system::System;

/// Record the owner and permissions of everything the item will change:
/// the key (for a value) or every key of the tree (for a key).
pub(super) fn record(system: &dyn System, item: &mut Item) -> Result<(), String> {
    let unreadable =
        |error: io::Error| format!("its permissions could not be read, so it was left: {error}");
    match (&item.value, &mut item.saved) {
        (Some(_), _) => {
            item.security = Some(system.key_security(&item.key).map_err(unreadable)?);
            Ok(())
        }
        (None, Some(Saved::Key(tree))) => record_tree(system, &item.key, tree).map_err(unreadable),
        (None, _) => Err("the backup is inconsistent".into()),
    }
}

fn record_tree(system: &dyn System, key: &RegPath, tree: &mut KeyTree) -> io::Result<()> {
    tree.security = Some(system.key_security(key)?);
    for child in &mut tree.keys {
        record_tree(system, &key.child(&child.name), &mut child.tree)?;
    }
    Ok(())
}

/// Take ownership and remove. Returns a warning when the entry was removed
/// but a remaining key's permissions could not be put back.
pub(super) fn remove(system: &dyn System, item: &Item) -> Result<Option<String>, String> {
    let refused = |what: &str, error: io::Error| format!("{what}: {error}");
    match (&item.value, &item.saved) {
        (Some(name), _) => {
            let original = item
                .security
                .as_deref()
                .ok_or("its permissions were not recorded")?;
            system
                .take_ownership(&item.key, false)
                .map_err(|error| refused("ownership could not be taken", error))?;
            let removed = system.delete_value(&item.key, name);
            let put_back = system.set_key_security(&item.key, original);
            removed.map_err(|error| refused("still refused after taking ownership", error))?;
            Ok(put_back.err().map(|error| {
                format!("its key's original permissions could not be put back: {error}")
            }))
        }
        (None, Some(Saved::Key(tree))) => {
            let taken = system
                .take_ownership(&item.key, true)
                .map_err(|error| refused("ownership could not be taken", error))
                .and_then(|()| {
                    system
                        .delete_key_tree(&item.key)
                        .map_err(|error| refused("still refused after taking ownership", error))
                });
            if taken.is_err() {
                // Leave what is still there as protected as it was.
                let _ = apply_security(system, &item.key, tree);
            }
            taken.map(|()| None)
        }
        (None, _) => Err("the backup is inconsistent".into()),
    }
}

/// Put recorded permissions back, deepest keys first, on keys that exist.
pub(super) fn apply_security(system: &dyn System, key: &RegPath, tree: &KeyTree) -> io::Result<()> {
    let mut first_error = None;
    for child in &tree.keys {
        if let Err(error) = apply_security(system, &key.child(&child.name), &child.tree) {
            first_error.get_or_insert(error);
        }
    }
    if let Some(sddl) = &tree.security
        && system.subkeys(key).is_ok()
        && let Err(error) = system.set_key_security(key, sddl)
    {
        first_error.get_or_insert(error);
    }
    first_error.map_or(Ok(()), Err)
}

/// Whether any key of the tree had its permissions recorded.
pub(super) fn has_security(tree: &KeyTree) -> bool {
    tree.security.is_some() || tree.keys.iter().any(|child| has_security(&child.tree))
}

/// Run `work`; if Windows refuses it with access denied, take ownership of
/// `key` for the duration and put its permissions back afterwards.
pub(super) fn with_owned<T>(
    system: &dyn System,
    key: &RegPath,
    work: impl Fn() -> io::Result<T>,
) -> io::Result<T> {
    match work() {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            let original = system.key_security(key)?;
            system.take_ownership(key, false)?;
            let done = work();
            let put_back = system.set_key_security(key, &original);
            let value = done?;
            put_back?;
            Ok(value)
        }
        other => other,
    }
}
