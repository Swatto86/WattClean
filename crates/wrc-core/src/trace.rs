//! Which program changed the registry. Windows' event tracing reports each
//! registry write with the process that made it, and a value's data
//! (with administrator rights, no driver); the watcher matches the changes
//! it finds to those writes (see `watch::explain`).

use std::io;

use serde::{Deserialize, Serialize};

use crate::format::{describe, value_name};
use crate::model::{RawValue, RegPath};
use crate::watch::{Change, Op};

/// The process behind a change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    pub pid: u32,
    /// The program's file name (`reg.exe`), or empty when Windows would
    /// not say.
    pub process: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOp {
    KeyCreated,
    KeyDeleted,
    ValueSet,
    ValueDeleted,
}

/// Value data as tracing captured it; Windows passes only the start of
/// large data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub kind: u32,
    /// The whole data's size in bytes.
    pub size: u32,
    pub bytes: Vec<u8>,
}

impl Captured {
    pub fn complete(&self) -> bool {
        usize::try_from(self.size).is_ok_and(|size| self.bytes.len() >= size)
    }

    /// The data as the log shows it, saying when only its start was seen.
    pub fn describe(&self) -> String {
        let shown = describe(&RawValue {
            name: String::new(),
            kind: self.kind,
            bytes: self.bytes.clone(),
        });
        if self.complete() {
            shown
        } else {
            format!(
                "{shown} … (first {} of {} bytes)",
                self.bytes.len(),
                self.size
            )
        }
    }
}

/// One registry write as tracing saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    /// Milliseconds since the Unix epoch.
    pub time: u64,
    pub actor: Actor,
    pub op: WriteOp,
    pub key: RegPath,
    /// The value's own name (empty for the default value); `None` for key
    /// operations.
    pub value: Option<String>,
    /// A value set: the data written.
    pub new: Option<Captured>,
    /// A value set over an existing value: what it held before.
    pub old: Option<Captured>,
}

impl Write {
    /// Set to exactly the data it already held: changes nothing.
    pub fn rewrote(&self) -> bool {
        self.op == WriteOp::ValueSet
            && match (&self.new, &self.old) {
                (Some(new), Some(old)) => new.complete() && old.complete() && new == old,
                _ => false,
            }
    }
}

/// A running trace of registry writes; dropping it stops tracing.
pub trait RegistryTrace: Send + Sync {
    /// Keep writes beneath these keys from now on, instead of the last list.
    fn follow(&self, keys: &[RegPath]);
    /// The writes kept from `from` to `to` (milliseconds since the Unix
    /// epoch), oldest first. Windows delivers events a little late, so this
    /// waits up to about a second for those made until `to`.
    fn writes(&self, from: u64, to: u64) -> Vec<Write>;
}

/// Whether the watcher can name the process behind each change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum TraceState {
    /// Tracing: changes are logged with their process.
    On,
    /// Nothing is being watched, so nothing is traced.
    Idle,
    /// Tracing needs administrator rights.
    NeedsAdmin,
    /// Windows refused to start tracing.
    Failed { problem: String },
}

/// Tracing could not start; what the page says instead of process names.
pub fn failure(error: &io::Error) -> TraceState {
    TraceState::Failed {
        problem: format!("Windows would not start event tracing: {error}"),
    }
}

/// Whether `write` is one that made `change`.
pub fn made(change: &Change, write: &Write) -> bool {
    let same_value = || {
        // Programs name keys in any case; the registry ignores it.
        write.key.folded() == change.key.folded()
            && write.value.as_deref().map(value_name).is_some_and(|name| {
                change
                    .value
                    .as_deref()
                    .is_some_and(|shown| shown.eq_ignore_ascii_case(name))
            })
    };
    match (change.op, write.op) {
        // Creating `A\B` in one call creates `A` too; a tree is deleted
        // from the bottom up.
        (Op::KeyCreated, WriteOp::KeyCreated) | (Op::KeyDeleted, WriteOp::KeyDeleted) => {
            write.key.is_within(&change.key)
        }
        (Op::ValueAdded | Op::ValueChanged, WriteOp::ValueSet)
        | (Op::ValueDeleted, WriteOp::ValueDeleted) => same_value(),
        _ => false,
    }
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use crate::model::Hive;

    fn key(path: &str) -> RegPath {
        RegPath::new(Hive::CurrentUser, path)
    }

    fn write(op: WriteOp, path: &str, value: Option<&str>, pid: u32) -> Write {
        Write {
            time: 1,
            actor: Actor {
                pid,
                process: format!("p{pid}.exe"),
            },
            op,
            key: key(path),
            value: value.map(str::to_owned),
            new: None,
            old: None,
        }
    }

    fn change(op: Op, path: &str, value: Option<&str>) -> Change {
        Change {
            op,
            key: key(path),
            value: value.map(str::to_owned),
            kind: None,
            old: None,
            new: None,
        }
    }

    /// The last of `writes` that made `change`.
    fn pid(change: &Change, writes: &[Write]) -> Option<u32> {
        writes
            .iter()
            .rev()
            .find(|write| made(change, write))
            .map(|write| write.actor.pid)
    }

    #[test]
    fn a_value_change_is_the_last_process_that_set_it() {
        let writes = [
            write(WriteOp::ValueSet, r"S\A", Some("Size"), 1),
            write(WriteOp::ValueSet, r"S\A", Some("size"), 2),
            write(WriteOp::ValueSet, r"S\A", Some("Other"), 3),
            write(WriteOp::ValueSet, r"S\B", Some("Size"), 4),
        ];
        let changed = change(Op::ValueChanged, r"s\a", Some("Size"));
        assert_eq!(pid(&changed, &writes), Some(2), "case is ignored");
        let added = change(Op::ValueAdded, r"S\A", Some("Other"));
        assert_eq!(pid(&added, &writes), Some(3));
    }

    #[test]
    fn the_default_value_matches_its_shown_name() {
        let writes = [write(WriteOp::ValueSet, "S", Some(""), 7)];
        assert_eq!(
            pid(&change(Op::ValueAdded, "S", Some("(Default)")), &writes),
            Some(7)
        );
    }

    #[test]
    fn keys_match_writes_beneath_them_and_ops_must_agree() {
        let writes = [
            write(WriteOp::KeyCreated, r"S\New\Deep", None, 5),
            write(WriteOp::ValueDeleted, r"S\Old", Some("X"), 6),
            write(WriteOp::KeyDeleted, r"S\Gone", None, 8),
        ];
        assert_eq!(
            pid(&change(Op::KeyCreated, r"S\New", None), &writes),
            Some(5)
        );
        assert_eq!(pid(&change(Op::KeyDeleted, r"S\New", None), &writes), None);
        assert_eq!(
            pid(&change(Op::ValueDeleted, r"S\Old", Some("X")), &writes),
            Some(6)
        );
        assert_eq!(
            pid(&change(Op::ValueChanged, r"S\Old", Some("X")), &writes),
            None
        );
        assert_eq!(
            pid(&change(Op::KeyDeleted, r"S\Gone", None), &writes),
            Some(8)
        );
    }
}
