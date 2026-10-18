//! A watched key's contents at one moment, and what changed between two
//! such moments.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::format::{describe, kind_name, value_name};
use crate::model::{RawValue, RegPath};
use crate::system::System;
use crate::{Error, Result};

/// A watched key may hold at most this many keys: each change re-reads the
/// whole tree, so a wider key would make every change slow to report.
pub const MAX_KEYS: usize = 20_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    keys: BTreeMap<String, (RegPath, Vec<RawValue>)>,
}

impl Snapshot {
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Read `root` and everything beneath it.
pub fn snapshot(system: &dyn System, root: &RegPath) -> Result<Snapshot> {
    let mut keys = BTreeMap::new();
    let mut stack = vec![root.clone()];
    while let Some(key) = stack.pop() {
        let values = match system.values(&key) {
            Ok(values) => values,
            // A subkey deleted while being read is simply no longer there.
            Err(error) if key != *root && error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(Error::registry("read", &key, source)),
        };
        for name in system.subkeys(&key).unwrap_or_default() {
            stack.push(key.child(&name));
        }
        keys.insert(key.folded(), (key, values));
        if keys.len() > MAX_KEYS {
            return Err(Error::Invalid(format!(
                "{} holds more than {MAX_KEYS} keys. Watch a key further down.",
                root.display()
            )));
        }
    }
    Ok(Snapshot { keys })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Op {
    KeyCreated,
    KeyDeleted,
    ValueAdded,
    ValueChanged,
    ValueDeleted,
    /// A value written whose data, before and after, tracing could not see.
    ValueSet,
}

/// One change, ready to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub op: Op,
    pub key: RegPath,
    pub value: Option<String>,
    pub kind: Option<String>,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// What changed from `before` to `after`, keys in path order.
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut changes = Vec::new();
    for (folded, (key, values)) in &after.keys {
        match before.keys.get(folded) {
            None => {
                changes.push(key_change(Op::KeyCreated, key));
                for value in values {
                    changes.push(value_change(Op::ValueAdded, key, None, Some(value)));
                }
            }
            Some((_, old_values)) => {
                for value in values {
                    let old = old_values
                        .iter()
                        .find(|old| old.name.eq_ignore_ascii_case(&value.name));
                    match old {
                        None => changes.push(value_change(Op::ValueAdded, key, None, Some(value))),
                        Some(old) if old != value => {
                            changes.push(value_change(
                                Op::ValueChanged,
                                key,
                                Some(old),
                                Some(value),
                            ));
                        }
                        Some(_) => {}
                    }
                }
                for old in old_values {
                    if !values
                        .iter()
                        .any(|value| value.name.eq_ignore_ascii_case(&old.name))
                    {
                        changes.push(value_change(Op::ValueDeleted, key, Some(old), None));
                    }
                }
            }
        }
    }
    for (folded, (key, _)) in &before.keys {
        if !after.keys.contains_key(folded) {
            changes.push(key_change(Op::KeyDeleted, key));
        }
    }
    changes
}

fn key_change(op: Op, key: &RegPath) -> Change {
    Change {
        op,
        key: key.clone(),
        value: None,
        kind: None,
        old: None,
        new: None,
    }
}

fn value_change(op: Op, key: &RegPath, old: Option<&RawValue>, new: Option<&RawValue>) -> Change {
    let shown = new.or(old);
    Change {
        op,
        key: key.clone(),
        value: shown.map(|value| value_name(&value.name).to_owned()),
        kind: shown.map(|value| kind_name(value.kind).to_owned()),
        old: old.map(describe),
        new: new.map(describe),
    }
}
