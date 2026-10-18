//! One watched key's thread: wait for Windows to signal a change, read the
//! key again, and log the difference with the process behind each change,
//! and every traced write the readings would miss (see `explain`).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::explain::{Reading, explain};
use super::log::Log;
use super::{diff, lock, snapshot};
use crate::model::RegPath;
use crate::system::System;
use crate::trace::{RegistryTrace, TraceState, Write};
use crate::{Error, Result};

/// How long a watch waits for Windows before checking whether to stop.
const POLL: Duration = Duration::from_millis(400);
/// Event times and the watcher's clock may disagree by this much (ms).
const SLACK: u64 = 250;

/// The running trace, shared with every watch thread.
pub(super) struct Tracing {
    pub trace: Option<Arc<dyn RegistryTrace>>,
    pub state: TraceState,
}

/// Watch `key` until told to stop, logging each change found with the
/// process behind it when tracing is on.
pub(super) fn follow(
    system: &dyn System,
    key: &RegPath,
    stop: &AtomicBool,
    log: &Mutex<Log>,
    tracing: &Mutex<Tracing>,
) -> Result<()> {
    let mut watch = system
        .watch_key(key)
        .map_err(|source| Error::registry("watch", key, source))?;
    let mut since = now();
    let mut last = snapshot(system, key)?;
    let mut seen = HashSet::new();
    // Writes left for a later reading: read again at the next poll even
    // when Windows signals nothing more.
    let mut waiting = false;
    while !stop.load(Ordering::Relaxed) {
        let changed = watch
            .wait(POLL)
            .map_err(|source| Error::registry("watch", key, source))?;
        if !changed && !waiting {
            continue;
        }
        let read = now();
        let next = snapshot(system, key)?;
        let changes = diff(&last, &next);
        // The changes were made after the last reading began. Readings that
        // match can still hide writes undone in between.
        let until = now();
        let writes = traced(tracing, key, since, until);
        if !changes.is_empty() || !writes.is_empty() {
            let reading = Reading {
                started: read,
                finished: until,
            };
            let explained = explain(changes, &writes, &seen, reading);
            seen = explained.logged;
            waiting = explained.waiting;
            if !explained.found.is_empty() {
                lock(log).record(explained.found);
            }
        } else {
            waiting = false;
        }
        since = read;
        last = next;
    }
    Ok(())
}

/// The writes beneath `key` traced between `from` and `to`; none when
/// tracing is off.
fn traced(tracing: &Mutex<Tracing>, key: &RegPath, from: u64, to: u64) -> Vec<Write> {
    // Not held while the trace waits for Windows' events.
    let trace = lock(tracing).trace.clone();
    trace
        .map(|trace| trace.writes(from.saturating_sub(SLACK), to.saturating_add(SLACK)))
        .unwrap_or_default()
        .into_iter()
        // The trace follows every watched key.
        .filter(|write| write.key.is_within(key))
        .collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
