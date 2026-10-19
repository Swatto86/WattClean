//! The log entries for one reading: each change found, with the process
//! behind it and the time of its write; and, when tracing is on, every
//! traced write the readings alone would miss, with its own data: several
//! writes to one value between two readings, or a change undone before the
//! next one (as a kernel driver such as Revo's sees them). A write that
//! leaves the data as it was is not logged.

use std::collections::HashSet;

use super::{Change, Op};
use crate::format::{kind_name, value_name};
use crate::trace::{Actor, Write, WriteOp, made};

/// One log entry, before it is numbered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub change: Change,
    pub actor: Option<Actor>,
    /// Milliseconds since the Unix epoch.
    pub time: u64,
}

/// When a reading of the key began and ended, in milliseconds since the
/// Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub started: u64,
    pub finished: u64,
}

/// A write this close to a reading's start may have missed it (the two
/// clocks differ a little): it waits for the next reading.
const UNSURE: u64 = 100;
/// Writes logged are remembered this long, as windows overlap.
const REMEMBERED: u64 = 5_000;

/// What one reading logs, and what it leaves for the next.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Explained {
    /// The entries, in time order.
    pub found: Vec<Found>,
    /// The writes logged now or lately, for the next reading's `seen`.
    pub logged: HashSet<WriteId>,
    /// Writes made too close to the reading to tell whether it shows them:
    /// another reading should follow soon.
    pub waiting: bool,
}

/// A write's identity, so one seen by two overlapping windows is logged once.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WriteId(u64, u32, String, Option<String>, u8);

fn id(write: &Write) -> WriteId {
    let op = match write.op {
        WriteOp::KeyCreated => 0,
        WriteOp::KeyDeleted => 1,
        WriteOp::ValueSet => 2,
        WriteOp::ValueDeleted => 3,
    };
    WriteId(
        write.time,
        write.actor.pid,
        write.key.folded(),
        write.value.clone(),
        op,
    )
}

/// The entries for `changes`, found by `reading`, given the `writes` traced
/// meanwhile; `seen` are the writes earlier readings logged. A write made
/// during or after the reading, which it may not show, is left for the
/// next one unless a change found now is its. Returns the entries in time
/// order, and the writes logged now or lately.
pub fn explain(
    changes: Vec<Change>,
    writes: &[Write],
    seen: &HashSet<WriteId>,
    reading: Reading,
) -> Explained {
    let detected = reading.finished;
    let settled = reading.started.saturating_sub(UNSURE);
    let fresh: Vec<&Write> = writes
        .iter()
        .filter(|write| !seen.contains(&id(write)) && !write.rewrote())
        .collect();
    let mut used = vec![false; fresh.len()];
    let mut found = Vec::new();
    for change in changes {
        // A write after the reading ended cannot be what it found.
        let matching: Vec<usize> = (0..fresh.len())
            .filter(|&at| fresh[at].time <= reading.finished && made(&change, fresh[at]))
            .collect();
        for &at in &matching {
            used[at] = true;
        }
        let several = matching.len() > 1
            && matches!(
                change.op,
                Op::ValueAdded | Op::ValueChanged | Op::ValueDeleted
            );
        if several {
            found.extend(each_write(&change, &matching, &fresh));
        } else {
            let last = matching.last().map(|&at| fresh[at]);
            found.push(Found {
                actor: last.map(|write| write.actor.clone()),
                time: last.map_or(detected, |write| write.time),
                change,
            });
        }
    }
    let mut logged: HashSet<WriteId> = seen
        .iter()
        .filter(|seen| seen.0.saturating_add(REMEMBERED) >= reading.started)
        .cloned()
        .collect();
    let mut waiting = false;
    for (at, write) in fresh.iter().enumerate() {
        if used[at] {
            logged.insert(id(write));
        } else if write.time > settled {
            waiting = true;
        } else {
            found.push(Found {
                change: from_write(write),
                actor: Some(write.actor.clone()),
                time: write.time,
            });
            logged.insert(id(write));
        }
    }
    found.sort_by_key(|entry| entry.time);
    Explained {
        found,
        logged,
        waiting,
    }
}

/// One entry per write to the changed value. The readings hold the whole
/// data, so the first entry's old data and the last one's new data come
/// from them; the rest is what tracing captured.
fn each_write(change: &Change, matching: &[usize], fresh: &[&Write]) -> Vec<Found> {
    let count = matching.len();
    matching
        .iter()
        .enumerate()
        .map(|(step, &at)| {
            let write = fresh[at];
            let mut entry = from_write(write);
            if step == 0 {
                entry.old.clone_from(&change.old);
                // The readings saw no value before the first write.
                if change.op == Op::ValueAdded {
                    entry.op = Op::ValueAdded;
                }
            }
            if step + 1 == count {
                entry.new.clone_from(&change.new);
                entry.kind.clone_from(&change.kind);
                if change.op == Op::ValueDeleted {
                    entry.op = Op::ValueDeleted;
                }
            }
            Found {
                change: entry,
                actor: Some(write.actor.clone()),
                time: write.time,
            }
        })
        .collect()
}

/// A write as a log entry, with the data tracing captured. Windows does not
/// always pass the data: a value set is then just "set".
fn from_write(write: &Write) -> Change {
    let op = match write.op {
        WriteOp::KeyCreated => Op::KeyCreated,
        WriteOp::KeyDeleted => Op::KeyDeleted,
        WriteOp::ValueDeleted => Op::ValueDeleted,
        WriteOp::ValueSet if write.old.is_some() => Op::ValueChanged,
        WriteOp::ValueSet if write.new.is_some() => Op::ValueAdded,
        WriteOp::ValueSet => Op::ValueSet,
    };
    let data = write.new.as_ref().or(write.old.as_ref());
    Change {
        op,
        key: write.key.clone(),
        value: write
            .value
            .as_deref()
            .map(|name| value_name(name).to_owned()),
        kind: data.map(|data| kind_name(data.kind).to_owned()),
        old: write.old.as_ref().map(crate::trace::Captured::describe),
        new: write.new.as_ref().map(crate::trace::Captured::describe),
    }
}

#[cfg(test)]
#[cfg(windows)]
mod tests;
