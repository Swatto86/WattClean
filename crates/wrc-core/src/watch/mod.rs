//! The Registry Watcher: keys the person chose are watched while the app
//! runs, and every change found is logged with its old and new data. Windows
//! signals that something beneath a key changed; the watcher then compares
//! the key with its last reading. With administrator rights, Windows' event
//! tracing names the process behind each change (see `crate::trace`). A
//! change undone before the next reading is not seen. Each key's log is
//! kept on disk too (see `log`).

mod explain;
mod export;
mod follow;
mod log;
pub mod offline;
mod snapshot;
#[cfg(test)]
#[cfg(windows)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

pub use export::{table, to_csv, to_text};
pub use snapshot::{Change, MAX_KEYS, Op, Snapshot, diff, snapshot};

use crate::model::RegPath;
use crate::store::{read_json, write_json};
use crate::system::System;
use crate::trace::{Actor, TraceState, failure};
use crate::{Error, Result};

pub const FILE: &str = "watch.json";
/// Each key's log keeps this many events; older ones are dropped.
pub const MAX_EVENTS: usize = 10_000;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub key: RegPath,
    pub paused: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WatchFile {
    version: u32,
    keys: Vec<Saved>,
}

/// A logged change, numbered so the page can ask for what is new.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub time: u64,
    #[serde(flatten)]
    pub change: Change,
    /// The process that made the change, when tracing could tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<Actor>,
}

/// One watched key as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchView {
    pub key: RegPath,
    pub paused: bool,
    pub events: usize,
    /// Why the watch stopped by itself, if it did.
    pub problem: Option<String>,
}

use follow::{Tracing, follow};
use log::Log;

struct Watch {
    key: RegPath,
    paused: bool,
    log: Arc<Mutex<Log>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

pub struct Watcher {
    system: Arc<dyn System>,
    dir: PathBuf,
    watches: Mutex<BTreeMap<String, Watch>>,
    tracing: Arc<Mutex<Tracing>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every update is a whole insert, push or field write.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Watcher {
    /// The saved watch list, started (except paused keys). A damaged list
    /// is moved aside to `watch.json.damaged` rather than stopping the app
    /// or being overwritten.
    pub fn start(system: Arc<dyn System>, dir: &Path) -> Result<Self> {
        let watcher = Self {
            system,
            dir: dir.to_path_buf(),
            watches: Mutex::new(BTreeMap::new()),
            tracing: Arc::new(Mutex::new(Tracing {
                trace: None,
                state: TraceState::Idle,
            })),
        };
        let file = dir.join(FILE);
        let saved: WatchFile = match read_json(&file) {
            Ok(saved) => saved.unwrap_or_default(),
            Err(Error::Unreadable { .. }) => {
                let aside = dir.join(format!("{FILE}.damaged"));
                std::fs::rename(&file, &aside)
                    .map_err(|source| Error::file("move aside", &file, source))?;
                WatchFile::default()
            }
            Err(error) => return Err(error),
        };
        for entry in saved.keys {
            let log = Log::load(dir, &entry.key);
            let watch = watcher.begin(entry.key, entry.paused, log);
            lock(&watcher.watches).insert(watch.key.folded(), watch);
        }
        watcher.retrace();
        Ok(watcher)
    }

    pub fn tracing(&self) -> TraceState {
        lock(&self.tracing).state.clone()
    }

    pub fn list(&self) -> Vec<WatchView> {
        lock(&self.watches)
            .values()
            .map(|watch| {
                let log = lock(&watch.log);
                WatchView {
                    key: watch.key.clone(),
                    paused: watch.paused,
                    events: log.events.len(),
                    problem: log.problem.clone(),
                }
            })
            .collect()
    }

    /// Start watching a key. It must exist and not be too large.
    pub fn add(&self, key: RegPath) -> Result<()> {
        if lock(&self.watches).contains_key(&key.folded()) {
            return Err(Error::Invalid(format!(
                "{} is already watched.",
                key.display()
            )));
        }
        snapshot(self.system.as_ref(), &key)?;
        let log = Log::fresh(&self.dir, &key);
        let watch = self.begin(key, false, log);
        lock(&self.watches).insert(watch.key.folded(), watch);
        self.retrace();
        self.save()
    }

    pub fn remove(&self, key: &RegPath) -> Result<()> {
        let watch = lock(&self.watches).remove(&key.folded());
        let Some(mut watch) = watch else {
            return Err(not_watched(key));
        };
        end(&mut watch);
        lock(&watch.log).forget();
        self.retrace();
        self.save()
    }

    /// Pause (`true`) or resume a watch. A resumed watch starts from the
    /// key as it is now.
    pub fn pause(&self, key: &RegPath, paused: bool) -> Result<()> {
        {
            let mut watches = lock(&self.watches);
            let watch = watches
                .get_mut(&key.folded())
                .ok_or_else(|| not_watched(key))?;
            if watch.paused != paused {
                end(watch);
                watch.paused = paused;
                lock(&watch.log).problem = None;
                if !paused {
                    let started = self.begin_with(watch.key.clone(), Arc::clone(&watch.log));
                    watch.stop = started.0;
                    watch.thread = Some(started.1);
                }
            }
        }
        self.retrace();
        self.save()
    }

    /// Events logged for `key` after `since`.
    pub fn events(&self, key: &RegPath, since: u64) -> Result<Vec<Event>> {
        let watches = lock(&self.watches);
        let watch = watches.get(&key.folded()).ok_or_else(|| not_watched(key))?;
        let log = lock(&watch.log);
        Ok(log
            .events
            .iter()
            .filter(|event| event.seq > since)
            .cloned()
            .collect())
    }

    pub fn clear(&self, key: &RegPath) -> Result<()> {
        let watches = lock(&self.watches);
        let watch = watches.get(&key.folded()).ok_or_else(|| not_watched(key))?;
        lock(&watch.log).clear();
        Ok(())
    }

    /// Stop every watch; called as the app exits.
    pub fn stop_all(&self) {
        let mut watches = lock(&self.watches);
        // Tell every watch first, so they wind down together.
        for watch in watches.values() {
            watch.stop.store(true, Ordering::Relaxed);
        }
        for watch in watches.values_mut() {
            end(watch);
        }
        drop(watches);
        let mut tracing = lock(&self.tracing);
        tracing.trace = None;
        tracing.state = TraceState::Idle;
    }

    /// Trace while anything is watched, following exactly the watched keys.
    /// A trace that could not start is tried again at the next change to
    /// the list.
    fn retrace(&self) {
        let keys: Vec<RegPath> = lock(&self.watches)
            .values()
            .filter(|watch| !watch.paused)
            .map(|watch| watch.key.clone())
            .collect();
        let mut tracing = lock(&self.tracing);
        if keys.is_empty() {
            // Dropping the last handle stops Windows' session.
            tracing.trace = None;
            tracing.state = TraceState::Idle;
            return;
        }
        if tracing.trace.is_none() {
            if !self.system.is_elevated() {
                tracing.state = TraceState::NeedsAdmin;
                return;
            }
            match self.system.trace_registry() {
                Ok(trace) => {
                    tracing.trace = Some(Arc::from(trace));
                    tracing.state = TraceState::On;
                }
                Err(error) => {
                    tracing.state = failure(&error);
                    return;
                }
            }
        }
        if let Some(trace) = &tracing.trace {
            trace.follow(&keys);
        }
    }

    fn save(&self) -> Result<()> {
        let keys = lock(&self.watches)
            .values()
            .map(|watch| Saved {
                key: watch.key.clone(),
                paused: watch.paused,
            })
            .collect();
        write_json(&self.dir.join(FILE), &WatchFile { version: 1, keys })
    }

    fn begin(&self, key: RegPath, paused: bool, log: Log) -> Watch {
        let log = Arc::new(Mutex::new(log));
        let (stop, thread) = if paused {
            (Arc::new(AtomicBool::new(true)), None)
        } else {
            let (stop, thread) = self.begin_with(key.clone(), Arc::clone(&log));
            (stop, Some(thread))
        };
        Watch {
            key,
            paused,
            log,
            stop,
            thread,
        }
    }

    fn begin_with(&self, key: RegPath, log: Arc<Mutex<Log>>) -> (Arc<AtomicBool>, JoinHandle<()>) {
        let stop = Arc::new(AtomicBool::new(false));
        let system = Arc::clone(&self.system);
        let flag = Arc::clone(&stop);
        let tracing = Arc::clone(&self.tracing);
        let thread = std::thread::spawn(move || {
            if let Err(error) = follow(system.as_ref(), &key, &flag, &log, &tracing) {
                lock(&log).problem = Some(error.to_string());
            }
        });
        (stop, thread)
    }
}

fn end(watch: &mut Watch) {
    watch.stop.store(true, Ordering::Relaxed);
    if let Some(thread) = watch.thread.take() {
        // A watch thread only waits on Windows with a short timeout, so it
        // ends within one poll; a panic in it has already been recorded.
        let _ = thread.join();
    }
}

fn not_watched(key: &RegPath) -> Error {
    Error::Invalid(format!("{} is not watched.", key.display()))
}
