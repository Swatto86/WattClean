//! A short record of past cleans, shredding and wipes, for the History page.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::store;

/// Older entries drop off.
const KEEP: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Activity {
    Clean,
    Shred,
    Wipe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Seconds since 1970 (UTC).
    pub at: u64,
    pub activity: Activity,
    /// What was cleaned, shredded or wiped, in a few words.
    pub summary: String,
    pub items: u64,
    pub bytes: u64,
    pub problems: u64,
    pub cancelled: bool,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Newest first.
pub fn load(path: &Path) -> Vec<Entry> {
    store::load(path).unwrap_or_default()
}

pub fn append(path: &Path, entry: Entry) -> std::io::Result<()> {
    let mut entries = load(path);
    entries.insert(0, entry);
    entries.truncate(KEEP);
    store::save(path, &entries)
}

pub fn clear(path: &Path) -> std::io::Result<()> {
    store::save(path, &Vec::<Entry>::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        for i in 0..(KEEP as u64 + 5) {
            append(
                &path,
                Entry {
                    at: i,
                    activity: Activity::Clean,
                    summary: String::new(),
                    items: i,
                    bytes: 0,
                    problems: 0,
                    cancelled: false,
                },
            )
            .unwrap();
        }
        let entries = load(&path);
        assert_eq!(entries.len(), KEEP);
        assert_eq!(entries[0].at, KEEP as u64 + 4);
        clear(&path).unwrap();
        assert!(load(&path).is_empty());
    }
}
