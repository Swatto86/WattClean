//! The person's preferences, saved as `settings.json` in the data folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::keyname::parse_key;
use crate::scan::{Category, Mode, Options};
use crate::store::{read_json, write_json};
use crate::{Error, Result};

pub const FILE: &str = "settings.json";
/// More entries than anyone keeps by hand; a longer list is refused.
const MAX_LIST: usize = 500;
/// A terabyte: more is not a limit anyone means.
const MAX_BACKUPS_MB: u32 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// How large the page's text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FontSize {
    Small,
    Normal,
    Large,
}

/// One entry of the exclude or include list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListEntry {
    /// As the person wrote it; checked with [`parse_key`] when saved.
    pub key: String,
    /// The entry also covers everything beneath the key.
    pub subkeys: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub theme: Theme,
    /// The mode the cleaner starts on.
    pub mode: Mode,
    /// The categories last ticked for a custom scan.
    pub custom: Vec<Category>,
    pub all_accounts: bool,
    /// Never offer entries naming files on USB, network or missing drives.
    pub exclude_removable: bool,
    /// Never offer entries naming files inside the Windows folder.
    pub exclude_windows: bool,
    /// Create a System Restore point before each clean.
    pub restore_point: bool,
    /// Offer to take ownership of protected keys when cleaning.
    pub force_delete: bool,
    /// Make a full registry backup before each clean (administrator only).
    pub full_backup: bool,
    pub font_size: FontSize,
    /// Where backups are kept, when not in the data folder; changed only by
    /// moving them (`backup::move_backups`), never by saving settings.
    pub backups_folder: Option<String>,
    /// Closing the window keeps the app (and the watcher) running in the
    /// notification area.
    pub keep_in_tray: bool,
    /// Delete cleaner and search backups older than this many days; 0
    /// keeps them. Hand backups are never deleted automatically.
    pub keep_backups_days: u32,
    /// Delete the oldest of those while all backups take more than this
    /// many megabytes; 0 sets no limit (`backup::trim`).
    pub keep_backups_mb: u32,
    pub excludes: Vec<ListEntry>,
    pub includes: Vec<ListEntry>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            theme: Theme::System,
            mode: Mode::Safe,
            custom: Category::for_mode(Mode::Moderate, &[]),
            all_accounts: false,
            exclude_removable: true,
            exclude_windows: true,
            restore_point: true,
            force_delete: false,
            full_backup: false,
            font_size: FontSize::Normal,
            backups_folder: None,
            keep_in_tray: false,
            keep_backups_days: 0,
            keep_backups_mb: 0,
            excludes: Vec::new(),
            includes: Vec::new(),
        }
    }
}

/// Where backups are kept, by the saved settings.
pub fn backups_dir(data_dir: &Path) -> PathBuf {
    Settings::load(data_dir)
        .unwrap_or_default()
        .backups_dir(data_dir)
}

impl Settings {
    /// Saved settings, or the defaults when none were saved yet. A damaged
    /// file is reported, not overwritten.
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(read_json(&dir.join(FILE))?.unwrap_or_default())
    }

    /// Check what the page sent, then save it.
    pub fn save(&self, dir: &Path) -> Result<()> {
        self.validate()?;
        write_json(&dir.join(FILE), self)
    }

    fn validate(&self) -> Result<()> {
        if self.keep_backups_days > 3650 {
            return Err(Error::Invalid(
                "Keep backups for at most 3650 days, or 0 to keep them all.".into(),
            ));
        }
        if self.keep_backups_mb > MAX_BACKUPS_MB {
            return Err(Error::Invalid(format!(
                "Limit backups to at most {MAX_BACKUPS_MB} MB, or 0 for no limit."
            )));
        }
        for (name, list) in [("exclude", &self.excludes), ("include", &self.includes)] {
            if list.len() > MAX_LIST {
                return Err(Error::Invalid(format!(
                    "The {name} list can hold at most {MAX_LIST} keys."
                )));
            }
            for entry in list {
                let key = parse_key(&entry.key).map_err(|reason| {
                    Error::Invalid(format!("{} in the {name} list: {reason}", entry.key))
                })?;
                if key.path.is_empty() {
                    return Err(Error::Invalid(format!(
                        "{} in the {name} list is a whole hive; name a key inside it.",
                        entry.key
                    )));
                }
            }
        }
        Ok(())
    }

    /// Where backups are kept: the chosen folder, or the data folder's.
    pub fn backups_dir(&self, data_dir: &Path) -> PathBuf {
        self.backups_folder
            .as_deref()
            .map(PathBuf::from)
            .filter(|folder| folder.is_absolute())
            .unwrap_or_else(|| data_dir.join("backups"))
    }

    /// The scan options these settings give.
    pub fn options(&self) -> Options {
        let keys = |list: &[ListEntry]| {
            list.iter()
                .filter(|entry| entry.enabled)
                .filter_map(|entry| parse_key(&entry.key).ok().map(|key| (key, entry.subkeys)))
                .collect()
        };
        Options {
            exclude_removable: self.exclude_removable,
            exclude_windows: self.exclude_windows,
            excludes: keys(&self.excludes),
            includes: keys(&self.includes),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Hive, RegPath};

    #[test]
    fn defaults_then_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()).unwrap(), Settings::default());
        let changed = Settings {
            theme: Theme::Dark,
            mode: Mode::Advanced,
            excludes: vec![ListEntry {
                key: r"HKCU\Software\Keep".into(),
                subkeys: true,
                enabled: true,
            }],
            ..Settings::default()
        };
        changed.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).unwrap(), changed);
        assert_eq!(
            changed.options().excludes,
            vec![(RegPath::new(Hive::CurrentUser, r"Software\Keep"), true)]
        );
    }

    #[test]
    fn refuses_bad_lists_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        for key in [r"HKCR\.txt", "HKCU", "nonsense"] {
            let bad = Settings {
                includes: vec![ListEntry {
                    key: key.into(),
                    subkeys: false,
                    enabled: true,
                }],
                ..Settings::default()
            };
            assert!(bad.save(dir.path()).is_err(), "{key}");
        }
        assert!(!dir.path().join(FILE).exists(), "nothing was written");
        assert!(serde_json::from_str::<Settings>(r#"{"deleteEverything":true}"#).is_err());
        assert!(serde_json::from_str::<Settings>(r#"{"mode":"reckless"}"#).is_err());
    }

    #[test]
    fn disabled_entries_do_not_apply() {
        let settings = Settings {
            includes: vec![ListEntry {
                key: r"HKCU\Software\Junk".into(),
                subkeys: false,
                enabled: false,
            }],
            ..Settings::default()
        };
        assert!(settings.options().includes.is_empty());
    }
}
