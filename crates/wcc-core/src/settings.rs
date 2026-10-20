//! The user's settings, stored as `settings.json` in the data folder.
//!
//! Changes arrive as a patch that names only known fields; every path is
//! checked before it is accepted. Loading keeps each field that parses, so
//! one bad value never resets the rest.

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::OptionKey;
use crate::ops::CustomPath;
use crate::store;
use crate::vars;

/// More entries than anyone keeps by hand; stops a runaway list.
const LIST_LIMIT: usize = 2_000;
const PATH_LIMIT: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    /// A named accent: sky, violet, pink, teal, green, amber or red.
    pub accent: String,
    /// List only cleaners whose program is on this PC.
    pub hide_irrelevant: bool,
    /// Overwrite file contents before deleting (slower; defeats recovery).
    pub shred: bool,
    /// Ask before cleaning.
    pub confirm: bool,
    /// Files in use are deleted at the next restart (administrator only).
    pub delete_on_reboot: bool,
    /// Use the imported winapp2.ini.
    pub community: bool,
    /// Close the app when a clean finishes (BleachBit's "Exit after cleaning").
    pub exit_after_clean: bool,
    /// Ticked options as `cleaner.option`; `None` until the first change,
    /// which means the built-in defaults.
    pub selection: Option<Vec<String>>,
    /// Files and folders never removed.
    pub keep: Vec<CustomPath>,
    /// Files and folders removed by System › Custom paths.
    pub custom_paths: Vec<CustomPath>,
    pub nav_collapsed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            accent: "sky".to_string(),
            hide_irrelevant: true,
            shred: false,
            confirm: true,
            delete_on_reboot: false,
            community: true,
            exit_after_clean: false,
            selection: None,
            keep: Vec::new(),
            custom_paths: Vec::new(),
            nav_collapsed: false,
        }
    }
}

/// A change to some settings. Unknown fields are refused.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub theme: Option<Theme>,
    pub accent: Option<String>,
    pub hide_irrelevant: Option<bool>,
    pub shred: Option<bool>,
    pub confirm: Option<bool>,
    pub delete_on_reboot: Option<bool>,
    pub community: Option<bool>,
    pub exit_after_clean: Option<bool>,
    pub selection: Option<Vec<String>>,
    pub keep: Option<Vec<CustomPath>>,
    pub custom_paths: Option<Vec<CustomPath>>,
    pub nav_collapsed: Option<bool>,
}

const ACCENTS: &[&str] = &["sky", "violet", "pink", "teal", "green", "amber", "red"];

/// What a full path looks like on this system, for messages.
const FULL_PATH_EXAMPLE: &str = if cfg!(windows) {
    r"C:\Folder"
} else if cfg!(target_os = "macos") {
    "/Users/you/Folder"
} else {
    "/home/you/Folder"
};

/// A path a person picked, taken exactly as given (Shred): absolute, on a
/// drive or share, sane length. It may hold `%`, which nothing expands. It
/// may not step back up with `..`: the guard folds that by the text, the
/// system by following links, so `Link/..` would reach a folder the guard
/// never looked at.
pub fn check_full_path(path: &str) -> Result<(), String> {
    if path.trim().is_empty() || path.len() > PATH_LIMIT || path.contains('\0') {
        return Err(format!("{path:?} is not a usable path"));
    }
    if !Path::new(path).is_absolute() {
        return Err(format!(
            "{path:?} must be a full path such as {FULL_PATH_EXAMPLE}"
        ));
    }
    if Path::new(path)
        .components()
        .any(|part| part == Component::ParentDir)
    {
        return Err(format!(
            "{path:?} steps back up with \"..\": give the folder itself, such as {FULL_PATH_EXAMPLE}"
        ));
    }
    Ok(())
}

/// A path for the keep list or Custom paths, without surrounding spaces. A
/// `%NAME%` in it is refused: nothing expands these lists, so it would
/// stand for a folder of that name, not the one it seems to mean.
pub fn check_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    check_full_path(trimmed)?;
    if vars::has_variable(trimmed) {
        return Err(format!(
            "{trimmed:?} holds a variable (%NAME%): give the full path itself, such as {FULL_PATH_EXAMPLE}"
        ));
    }
    Ok(())
}

fn check_list(list: &[CustomPath], what: &str) -> Result<(), String> {
    if list.len() > LIST_LIMIT {
        return Err(format!("the {what} holds more than {LIST_LIMIT} entries"));
    }
    list.iter().try_for_each(|entry| check_path(&entry.path))
}

impl SettingsPatch {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(accent) = &self.accent
            && !ACCENTS.contains(&accent.as_str())
        {
            return Err(format!("unknown accent {accent:?}"));
        }
        if let Some(selection) = &self.selection
            && (selection.len() > 50_000 || selection.iter().any(|s| OptionKey::parse(s).is_none()))
        {
            return Err("the selection holds an entry that is not cleaner.option".into());
        }
        if let Some(keep) = &self.keep {
            check_list(keep, "keep list")?;
            // The guard compares keep entries literally, so a pattern would
            // protect nothing while looking as if it did.
            if let Some(entry) = keep.iter().find(|e| e.path.contains(['*', '?'])) {
                return Err(format!(
                    "{:?} has a wildcard (* or ?): the keep list takes exact files and folders",
                    entry.path
                ));
            }
        }
        if let Some(custom) = &self.custom_paths {
            check_list(custom, "custom path list")?;
        }
        Ok(())
    }
}

/// A stored keep list minus entries with wildcards (an older version took
/// them, but never honoured them); the other entries stay.
fn without_wildcards(value: Value) -> Value {
    match value {
        Value::Array(entries) => Value::Array(
            entries
                .into_iter()
                .filter(|e| {
                    !e.get("path")
                        .and_then(Value::as_str)
                        .is_some_and(|p| p.contains(['*', '?']))
                })
                .collect(),
        ),
        other => other,
    }
}

impl Settings {
    /// Applies a validated patch.
    pub fn apply(&mut self, patch: SettingsPatch) -> Result<(), String> {
        patch.validate()?;
        let trim = |list: Vec<CustomPath>| -> Vec<CustomPath> {
            let mut out: Vec<CustomPath> = Vec::new();
            for mut entry in list {
                entry.path = entry.path.trim().to_string();
                if !out.iter().any(|e| e.path.eq_ignore_ascii_case(&entry.path)) {
                    out.push(entry);
                }
            }
            out
        };
        if let Some(v) = patch.theme {
            self.theme = v;
        }
        if let Some(v) = patch.accent {
            self.accent = v;
        }
        if let Some(v) = patch.hide_irrelevant {
            self.hide_irrelevant = v;
        }
        if let Some(v) = patch.shred {
            self.shred = v;
        }
        if let Some(v) = patch.confirm {
            self.confirm = v;
        }
        if let Some(v) = patch.delete_on_reboot {
            self.delete_on_reboot = v;
        }
        if let Some(v) = patch.community {
            self.community = v;
        }
        if let Some(v) = patch.exit_after_clean {
            self.exit_after_clean = v;
        }
        if let Some(mut v) = patch.selection {
            v.sort();
            v.dedup();
            self.selection = Some(v);
        }
        if let Some(v) = patch.keep {
            self.keep = trim(v);
        }
        if let Some(v) = patch.custom_paths {
            self.custom_paths = trim(v);
        }
        if let Some(v) = patch.nav_collapsed {
            self.nav_collapsed = v;
        }
        Ok(())
    }

    /// The stored settings, keeping each field that parses and validates.
    pub fn load(path: &Path) -> Self {
        let mut settings = Self::default();
        let Some(Value::Object(fields)) = store::load::<Value>(path) else {
            return settings;
        };
        for (name, value) in fields {
            let value = if name == "keep" {
                without_wildcards(value)
            } else {
                value
            };
            let single = Value::Object([(name, value)].into_iter().collect());
            if let Ok(patch) = serde_json::from_value::<SettingsPatch>(single) {
                // A bad field is dropped; the rest still apply.
                let _applied = settings.apply(patch);
            }
        }
        settings
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        store::save(path, self)
    }

    /// The selection as option keys, or `defaults` before the first change.
    pub fn selected(&self, defaults: &[OptionKey]) -> Vec<OptionKey> {
        match &self.selection {
            Some(list) => list.iter().filter_map(|s| OptionKey::parse(s)).collect(),
            None => defaults.to_vec(),
        }
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
