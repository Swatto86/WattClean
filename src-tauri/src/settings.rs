//! The shell's own settings, `settings.json` in WattClean's data folder: the
//! look, the navigation pane, and whether closing the window leaves the app
//! in the notification area. Each module keeps its own settings in its
//! folder (`disk`, `registry`, `uninstall`).
//!
//! Changes arrive as a patch naming only known fields. Loading keeps every
//! field that parses, so one bad value never resets the rest.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextSize {
    Small,
    #[default]
    Normal,
    Large,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSettings {
    pub theme: Theme,
    /// A named accent: sky, violet, pink, teal, green, amber or red.
    pub accent: String,
    pub text_size: TextSize,
    pub nav_collapsed: bool,
    /// Closing the window hides it to the notification area, so the
    /// Registry Watcher goes on watching; Quit is in the icon's menu.
    pub keep_in_tray: bool,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            accent: "sky".to_string(),
            text_size: TextSize::Normal,
            nav_collapsed: false,
            keep_in_tray: true,
        }
    }
}

/// A change to some settings. Unknown fields are refused.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShellPatch {
    pub theme: Option<Theme>,
    pub accent: Option<String>,
    pub text_size: Option<TextSize>,
    pub nav_collapsed: Option<bool>,
    pub keep_in_tray: Option<bool>,
}

const ACCENTS: &[&str] = &["sky", "violet", "pink", "teal", "green", "amber", "red"];

fn field<T: DeserializeOwned>(map: &Map<String, Value>, name: &str, into: &mut T) {
    if let Some(value) = map.get(name)
        && let Ok(parsed) = serde_json::from_value(value.clone())
    {
        *into = parsed;
    }
}

impl ShellSettings {
    /// The saved settings; the defaults for a missing or unreadable file,
    /// and for each field that does not parse.
    pub fn load(path: &Path) -> Self {
        let mut settings = Self::default();
        let Some(Value::Object(map)) = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
        else {
            return settings;
        };
        field(&map, "theme", &mut settings.theme);
        field(&map, "accent", &mut settings.accent);
        field(&map, "textSize", &mut settings.text_size);
        field(&map, "navCollapsed", &mut settings.nav_collapsed);
        field(&map, "keepInTray", &mut settings.keep_in_tray);
        if !ACCENTS.contains(&settings.accent.as_str()) {
            settings.accent = Self::default().accent;
        }
        settings
    }

    /// Written to a temporary file and renamed, so an interrupted save
    /// leaves the previous file readable.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        wcc_core::store::save(path, self)
    }

    /// These settings with the patch applied, or why it was refused.
    pub fn patched(&self, patch: ShellPatch) -> Result<Self, String> {
        let mut next = self.clone();
        if let Some(theme) = patch.theme {
            next.theme = theme;
        }
        if let Some(accent) = patch.accent {
            if !ACCENTS.contains(&accent.as_str()) {
                return Err(format!("unknown accent {accent:?}"));
            }
            next.accent = accent;
        }
        if let Some(size) = patch.text_size {
            next.text_size = size;
        }
        if let Some(collapsed) = patch.nav_collapsed {
            next.nav_collapsed = collapsed;
        }
        if let Some(keep) = patch.keep_in_tray {
            next.keep_in_tray = keep;
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_keeps_every_field_that_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"theme":"light","accent":"neon","textSize":12,"keepInTray":false}"#,
        )
        .unwrap();
        let settings = ShellSettings::load(&path);
        assert_eq!(settings.theme, Theme::Light);
        assert_eq!(settings.accent, "sky", "an unknown accent falls back");
        assert_eq!(settings.text_size, TextSize::Normal);
        assert!(!settings.keep_in_tray);
        assert_eq!(
            ShellSettings::load(&dir.path().join("missing.json")),
            ShellSettings::default()
        );
    }

    #[test]
    fn a_patch_names_known_fields_and_checks_the_accent() {
        let unknown = serde_json::from_str::<ShellPatch>(r#"{"colour":"red"}"#);
        assert!(unknown.is_err());
        let base = ShellSettings::default();
        let bad = ShellPatch {
            accent: Some("neon".into()),
            ..ShellPatch::default()
        };
        assert!(base.patched(bad).is_err());
        let good = ShellPatch {
            accent: Some("violet".into()),
            keep_in_tray: Some(false),
            ..ShellPatch::default()
        };
        let next = base.patched(good).unwrap();
        assert_eq!(next.accent, "violet");
        assert!(!next.keep_in_tray);
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = ShellSettings {
            theme: Theme::Dark,
            text_size: TextSize::Large,
            nav_collapsed: true,
            ..ShellSettings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(ShellSettings::load(&path), settings);
    }
}
