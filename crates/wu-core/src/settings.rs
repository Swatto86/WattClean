//! The person's preferences, saved as `settings.json` in the data folder.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::scan::Mode;
use crate::store::{read_json, write_json};

pub const FILE: &str = "settings.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    /// The leftover scan mode the uninstall wizard starts on.
    pub mode: Mode,
    /// Create a System Restore point before each uninstall.
    pub restore_point: bool,
    pub show_system_components: bool,
    /// Ask uninstallers to run without their own questions, where possible.
    pub quiet: bool,
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            mode: Mode::Moderate,
            restore_point: true,
            show_system_components: false,
            quiet: false,
            theme: Theme::System,
        }
    }
}

impl Settings {
    /// Saved settings, or the defaults when none were saved yet. A damaged
    /// file is reported, not overwritten.
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(read_json(&dir.join(FILE))?.unwrap_or_default())
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        write_json(&dir.join(FILE), self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_then_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()).unwrap(), Settings::default());
        let changed = Settings {
            mode: Mode::Advanced,
            theme: Theme::Dark,
            ..Settings::default()
        };
        changed.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).unwrap(), changed);
    }

    #[test]
    fn rejects_unknown_values_from_the_page() {
        let bad = r#"{"mode":"reckless"}"#;
        assert!(serde_json::from_str::<Settings>(bad).is_err());
        let extra = r#"{"deleteEverything":true}"#;
        assert!(serde_json::from_str::<Settings>(extra).is_err());
    }
}
