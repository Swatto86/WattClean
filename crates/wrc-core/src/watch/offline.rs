//! The watch list and logs while the app is closed, for the command line:
//! it changes `watch.json` and reads `watch-logs/` directly, and the app
//! picks the list up at its next start. Changing the list while the app
//! runs would be overwritten by it, so the command line refuses then.

use std::path::Path;

use super::log::Log;
use super::{Event, FILE, Saved, WatchFile, snapshot};
use crate::model::RegPath;
use crate::store::{read_json, write_json};
use crate::system::System;
use crate::{Error, Result};

/// The saved watch list.
pub fn saved(dir: &Path) -> Result<Vec<Saved>> {
    let file: Option<WatchFile> = read_json(&dir.join(FILE))?;
    Ok(file.unwrap_or_default().keys)
}

fn store(dir: &Path, keys: Vec<Saved>) -> Result<()> {
    write_json(&dir.join(FILE), &WatchFile { version: 1, keys })
}

fn position(keys: &[Saved], key: &RegPath) -> Option<usize> {
    keys.iter()
        .position(|saved| saved.key.folded() == key.folded())
}

/// Start watching `key` (it must exist and not be too large), or resume a
/// paused watch. A new watch starts with an empty log.
pub fn start(system: &dyn System, dir: &Path, key: &RegPath) -> Result<()> {
    let mut keys = saved(dir)?;
    match position(&keys, key) {
        Some(at) if !keys[at].paused => {
            return Err(Error::Invalid(format!(
                "{} is already watched.",
                key.display()
            )));
        }
        Some(at) => keys[at].paused = false,
        None => {
            snapshot(system, key)?;
            Log::fresh(dir, key);
            keys.push(Saved {
                key: key.clone(),
                paused: false,
            });
        }
    }
    store(dir, keys)
}

/// Pause a watch, keeping its log.
pub fn stop(dir: &Path, key: &RegPath) -> Result<()> {
    let mut keys = saved(dir)?;
    let at = position(&keys, key).ok_or_else(|| super::not_watched(key))?;
    keys[at].paused = true;
    store(dir, keys)
}

/// Stop watching `key` and delete its log.
pub fn delete(dir: &Path, key: &RegPath) -> Result<()> {
    let mut keys = saved(dir)?;
    let at = position(&keys, key).ok_or_else(|| super::not_watched(key))?;
    keys.remove(at);
    Log::load(dir, key).forget();
    store(dir, keys)
}

/// The changes logged for `key`, oldest first.
pub fn events(dir: &Path, key: &RegPath) -> Result<Vec<Event>> {
    if position(&saved(dir)?, key).is_none() {
        return Err(super::not_watched(key));
    }
    Ok(Log::load(dir, key).events.into_iter().collect())
}
