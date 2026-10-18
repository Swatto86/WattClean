//! Reading a key with everything beneath it, and writing such a copy back.

use serde::{Deserialize, Serialize};

use crate::model::{RawValue, RegPath};
use crate::system::System;
use crate::{Error, Result};

/// A backup refuses keys holding more than this many keys and values; a
/// cleaner never removes anything that large, and a hand backup that big
/// belongs in a System Restore point.
const MAX_ENTRIES: usize = 250_000;
/// The registry's own nesting limit.
const MAX_DEPTH: usize = 512;

/// A key's values and subkeys, exactly as stored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyTree {
    pub values: Vec<RawValue>,
    pub keys: Vec<Child>,
    /// Owner and permissions (SDDL), kept when the app had to take
    /// ownership to remove the key, so a restore protects it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Child {
    pub name: String,
    pub tree: KeyTree,
}

impl KeyTree {
    /// Keys and values in the tree, the key itself included.
    pub fn entries(&self) -> usize {
        1 + self.values.len()
            + self
                .keys
                .iter()
                .map(|child| child.tree.entries())
                .sum::<usize>()
    }
}

/// Copy `key` and everything beneath it. Fails rather than returning a
/// partial copy: a value or subkey that cannot be read stops the backup,
/// and with it the removal.
pub fn capture_key(system: &dyn System, key: &RegPath) -> Result<KeyTree> {
    let mut budget = MAX_ENTRIES;
    capture(system, key, 0, &mut budget)
}

fn capture(
    system: &dyn System,
    key: &RegPath,
    depth: usize,
    budget: &mut usize,
) -> Result<KeyTree> {
    if depth > MAX_DEPTH {
        return Err(Error::Invalid(format!(
            "{} is nested too deeply to back up",
            key.display()
        )));
    }
    let values = system
        .values(key)
        .map_err(|source| Error::registry("read", key, source))?;
    let names = system
        .subkeys(key)
        .map_err(|source| Error::registry("list", key, source))?;
    let used = 1 + values.len();
    if used > *budget {
        return Err(Error::Invalid(format!(
            "{} holds too much to back up (more than {MAX_ENTRIES} keys and values)",
            key.display()
        )));
    }
    *budget -= used;
    let mut keys = Vec::with_capacity(names.len());
    for name in names {
        let tree = capture(system, &key.child(&name), depth + 1, budget)?;
        keys.push(Child { name, tree });
    }
    Ok(KeyTree {
        values,
        keys,
        security: None,
    })
}

/// Create `key` and write the tree into it. With `only_missing`, values
/// already there are left as they are; returns how many were left that way.
pub fn write_tree(
    system: &dyn System,
    key: &RegPath,
    tree: &KeyTree,
    only_missing: bool,
) -> Result<usize> {
    system
        .create_key(key)
        .map_err(|source| Error::registry("create", key, source))?;
    let existing: Vec<String> = if only_missing {
        system
            .values(key)
            .map_err(|source| Error::registry("read", key, source))?
            .into_iter()
            .map(|value| value.name.to_lowercase())
            .collect()
    } else {
        Vec::new()
    };
    let mut left = 0;
    for value in &tree.values {
        if existing.contains(&value.name.to_lowercase()) {
            left += 1;
            continue;
        }
        system
            .write_value(key, value)
            .map_err(|source| Error::registry("write", key, source))?;
    }
    for child in &tree.keys {
        left += write_tree(system, &key.child(&child.name), &child.tree, only_missing)?;
    }
    Ok(left)
}
