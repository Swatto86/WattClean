//! A watched key's log: in memory for the page, and in
//! `watch-logs/<name>.jsonl` (one change per line, appended as it happens)
//! so it survives a restart. A file grown past its limit is rewritten with
//! only the newest changes that fit in half of it.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::explain::Found;
use super::{Event, MAX_EVENTS};
use crate::model::RegPath;

pub const FOLDER: &str = "watch-logs";
/// A log file is rewritten once it is larger than this.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct Log {
    pub events: VecDeque<Event>,
    pub next: u64,
    /// Why the watch stopped by itself, or its log could not be saved.
    pub problem: Option<String>,
    file: Option<PathBuf>,
    limit: u64,
}

/// The file name for a key: a hash of its folded path, so any key name
/// makes a valid, stable file name.
fn file_name(key: &RegPath) -> String {
    // FNV-1a, 64 bits.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.folded().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}.jsonl")
}

impl Log {
    /// The log saved for `key`, or an empty one. Lines that cannot be read
    /// are skipped.
    pub fn load(dir: &Path, key: &RegPath) -> Self {
        let file = dir.join(FOLDER).join(file_name(key));
        let mut events: VecDeque<Event> = fs::read_to_string(&file)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        while events.len() > MAX_EVENTS {
            events.pop_front();
        }
        let next = events.iter().map(|event| event.seq).max().unwrap_or(0);
        Self {
            events,
            next,
            problem: None,
            file: Some(file),
            limit: MAX_BYTES,
        }
    }

    /// An empty log for a key just added, replacing any old file.
    pub fn fresh(dir: &Path, key: &RegPath) -> Self {
        let mut log = Self::load(dir, key);
        log.clear();
        log
    }

    #[cfg(test)]
    pub fn with_limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }

    pub fn record(&mut self, found: Vec<Found>) {
        let mut lines = String::new();
        for Found {
            change,
            actor,
            time,
        } in found
        {
            self.next += 1;
            let event = Event {
                seq: self.next,
                time,
                change,
                actor,
            };
            if let Ok(line) = serde_json::to_string(&event) {
                lines.push_str(&line);
                lines.push('\n');
            }
            self.events.push_back(event);
            if self.events.len() > MAX_EVENTS {
                self.events.pop_front();
            }
        }
        if !lines.is_empty() {
            self.append(&lines);
        }
    }

    fn append(&mut self, lines: &str) {
        let Some(file) = &self.file else { return };
        let written = fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))
            .and_then(|()| OpenOptions::new().create(true).append(true).open(file))
            .and_then(|mut handle| handle.write_all(lines.as_bytes()));
        if let Err(error) = written {
            self.problem = Some(format!("The log could not be saved: {error}"));
            return;
        }
        if fs::metadata(file).is_ok_and(|meta| meta.len() > self.limit) {
            self.rewrite();
        }
    }

    /// Write the file again with the newest changes that fit in half the
    /// limit, dropping older ones from memory too, so the file is not
    /// rewritten again at the next change.
    fn rewrite(&mut self) {
        let Some(file) = &self.file else { return };
        let budget = usize::try_from(self.limit / 2).unwrap_or(usize::MAX);
        let mut kept: Vec<String> = Vec::new();
        let mut size = 0;
        for event in self.events.iter().rev() {
            let Ok(line) = serde_json::to_string(event) else {
                continue;
            };
            if size + line.len() + 1 > budget {
                break;
            }
            size += line.len() + 1;
            kept.push(line);
        }
        while self.events.len() > kept.len() {
            self.events.pop_front();
        }
        kept.reverse();
        let mut text = kept.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        if let Err(error) = crate::store::write_atomic(file, text.as_bytes()) {
            self.problem = Some(format!("The log could not be saved: {error}"));
        }
    }

    /// Forget every change, in memory and on disk.
    pub fn clear(&mut self) {
        self.events.clear();
        if let Some(file) = &self.file
            && file.exists()
            && let Err(error) = fs::write(file, "")
        {
            self.problem = Some(format!("The log could not be cleared: {error}"));
        }
    }

    /// Delete the log file, for a key no longer watched.
    pub fn forget(&self) {
        if let Some(file) = &self.file {
            let _ = fs::remove_file(file);
        }
    }
}
