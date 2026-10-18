//! A record of every uninstall, saved as `history.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::scan::Mode;
use crate::store::{read_json, write_json};

pub const FILE: &str = "history.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub keys: u32,
    pub values: u32,
    pub folders: u32,
    pub files: u32,
}

impl Counts {
    pub fn total(&self) -> u32 {
        self.keys + self.values + self.folders + self.files
    }
}

/// What happened to the program's own uninstaller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Uninstaller {
    Finished,
    Failed {
        error: String,
    },
    /// Forced uninstall: no uninstaller was run.
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    /// Seconds since the Unix epoch.
    pub time: u64,
    pub program: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    /// The leftover scan, or `None` when only the uninstaller ran.
    pub mode: Option<Mode>,
    pub uninstaller: Uninstaller,
    pub removed: Counts,
    pub at_restart: u32,
    pub failed: u32,
    pub bytes: u64,
    pub backup_id: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    version: u32,
    entries: Vec<Entry>,
}

/// Newest first.
pub fn load(dir: &Path) -> Result<Vec<Entry>> {
    let file: File = read_json(&dir.join(FILE))?.unwrap_or_default();
    let mut entries = file.entries;
    entries.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| b.id.cmp(&a.id)));
    Ok(entries)
}

pub fn append(dir: &Path, entry: Entry) -> Result<()> {
    let mut entries = load(dir)?;
    entries.retain(|known| known.id != entry.id);
    entries.push(entry);
    write_json(
        &dir.join(FILE),
        &File {
            version: 1,
            entries,
        },
    )
}

pub fn clear(dir: &Path) -> Result<()> {
    // Refuse to clear over a file that cannot be read, like every other write.
    load(dir)?;
    write_json(
        &dir.join(FILE),
        &File {
            version: 1,
            entries: Vec::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, time: u64) -> Entry {
        Entry {
            id: id.into(),
            time,
            program: "Acme".into(),
            publisher: None,
            version: None,
            mode: Some(Mode::Safe),
            uninstaller: Uninstaller::Finished,
            removed: Counts::default(),
            at_restart: 0,
            failed: 0,
            bytes: 0,
            backup_id: None,
        }
    }

    #[test]
    fn appends_newest_first_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), entry("a", 10)).unwrap();
        append(dir.path(), entry("b", 20)).unwrap();
        let ids: Vec<String> = load(dir.path())
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["b", "a"]);
        clear(dir.path()).unwrap();
        assert!(load(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn a_damaged_history_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "garbage").unwrap();
        assert!(append(dir.path(), entry("a", 1)).is_err());
        assert!(clear(dir.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(FILE)).unwrap(),
            "garbage"
        );
    }
}
