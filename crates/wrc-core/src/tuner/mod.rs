//! The Registry Tuner: read whether each tweak is on, switch it, and put
//! back exactly what was there before the app changed it. What it changed
//! is recorded in `tuner.json` before anything is written.

mod catalog;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use catalog::{Change, Data, Group, Needs, TWEAKS, Tweak};

use crate::backup::{KeyTree, capture_key, write_tree};
use crate::keyname::parse_key;
use crate::model::{Hive, RawValue, RegPath, find_value};
use crate::store::{read_json, write_json};
use crate::system::{System, key_exists, text_value, values_or_empty};
use crate::{Error, Result};

pub const FILE: &str = "tuner.json";
/// Windows 11's first build number.
const WINDOWS_11: u32 = 22_000;

/// What the registry held before the app changed it, to put back exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Prior {
    Value {
        key: RegPath,
        name: String,
        old: Option<RawValue>,
    },
    /// A key the change created; reverting deletes it with its contents.
    CreatedKey {
        key: RegPath,
    },
    Key {
        key: RegPath,
        old: Option<KeyTree>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub version: u32,
    /// Per tweak id, what was there before the app switched it on.
    pub changed: BTreeMap<String, Vec<Prior>>,
}

impl Record {
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(read_json(&dir.join(FILE))?.unwrap_or(Self {
            version: 1,
            ..Self::default()
        }))
    }

    fn save(&self, dir: &Path) -> Result<()> {
        write_json(&dir.join(FILE), self)
    }
}

/// One tweak as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TweakView {
    pub id: &'static str,
    pub group: Group,
    pub title: &'static str,
    pub description: &'static str,
    pub needs: Needs,
    /// Changes machine-wide settings, so needs administrator rights.
    pub machine: bool,
    pub on: bool,
    /// Switched on by this app, which can put back what was there.
    pub changed_by_app: bool,
    /// Why it cannot be switched here, if it cannot.
    pub unavailable: Option<String>,
}

/// Every tweak with its current state.
pub fn list(system: &dyn System, dir: &Path) -> Result<Vec<TweakView>> {
    let record = Record::load(dir)?;
    let build = windows_build(system);
    TWEAKS
        .iter()
        .map(|tweak| view(system, tweak, &record, build))
        .collect()
}

/// Switch a tweak on or off and return its new state. Off puts back what
/// was there before the app switched it on; a tweak that was on already
/// gets Windows' default.
pub fn set(system: &dyn System, dir: &Path, id: &str, on: bool) -> Result<TweakView> {
    let tweak = find(id)?;
    let mut record = Record::load(dir)?;
    let build = windows_build(system);
    if let Some(reason) = view(system, tweak, &record, build)?.unavailable {
        return Err(Error::Invalid(reason));
    }
    if on {
        if !is_on(system, tweak)? {
            if !record.changed.contains_key(tweak.id) {
                let priors = capture(system, tweak.on)?;
                record.changed.insert(tweak.id.to_owned(), priors);
                record.save(dir)?;
            }
            apply(system, tweak.on)?;
        }
    } else if let Some(priors) = record.changed.get(tweak.id).cloned() {
        revert(system, &priors)?;
        record.changed.remove(tweak.id);
        record.save(dir)?;
    } else {
        apply(system, tweak.off)?;
    }
    view(system, tweak, &record, build)
}

/// Switch off every tweak the app switched on. Returns how many were reset
/// and the ones that could not be, with the reason.
pub fn reset_all(system: &dyn System, dir: &Path) -> Result<(usize, Vec<(String, String)>)> {
    let ids: Vec<String> = Record::load(dir)?.changed.keys().cloned().collect();
    let mut reset = 0;
    let mut failed = Vec::new();
    for id in ids {
        match set(system, dir, &id, false) {
            Ok(_) => reset += 1,
            Err(error) => {
                let title = find(&id).map_or_else(|_| id.clone(), |tweak| tweak.title.to_owned());
                failed.push((title, error.to_string()));
            }
        }
    }
    Ok((reset, failed))
}

fn find(id: &str) -> Result<&'static Tweak> {
    TWEAKS
        .iter()
        .find(|tweak| tweak.id == id)
        .ok_or_else(|| Error::Invalid(format!("There is no tweak \"{id}\".")))
}

fn view(
    system: &dyn System,
    tweak: &'static Tweak,
    record: &Record,
    build: Option<u32>,
) -> Result<TweakView> {
    let machine = touches_machine(tweak)?;
    let unavailable = if tweak.windows11 && build.is_some_and(|build| build < WINDOWS_11) {
        Some("Only Windows 11 has this setting.".to_owned())
    } else if tweak.windows10 && build.is_some_and(|build| build >= WINDOWS_11) {
        Some("Only Windows 10 has this setting.".to_owned())
    } else if machine && !system.is_elevated() {
        Some("Changing this needs administrator rights.".to_owned())
    } else {
        None
    };
    Ok(TweakView {
        id: tweak.id,
        group: tweak.group,
        title: tweak.title,
        description: tweak.description,
        needs: tweak.needs,
        machine,
        on: is_on(system, tweak)?,
        changed_by_app: record.changed.contains_key(tweak.id),
        unavailable,
    })
}

fn touches_machine(tweak: &Tweak) -> Result<bool> {
    for change in tweak.on.iter().chain(tweak.off) {
        if key_of(change)?.hive != Hive::CurrentUser {
            return Ok(true);
        }
    }
    Ok(false)
}

fn key_of(change: &Change) -> Result<RegPath> {
    let text = match change {
        Change::Set(key, _, _) | Change::Remove(key, _) | Change::RemoveKey(key) => key,
    };
    parse_key(text).map_err(Error::Invalid)
}

fn raw(name: &str, data: Data) -> RawValue {
    match data {
        Data::Dword(number) => RawValue::dword(name, number),
        Data::Text(text) => RawValue::string(name, text),
        Data::Expand(text) => RawValue::expand_string(name, text),
    }
}

/// True when every change of the tweak's "on" set is in effect.
fn is_on(system: &dyn System, tweak: &Tweak) -> Result<bool> {
    for change in tweak.on {
        let key = key_of(change)?;
        let holds = match change {
            Change::Set(_, name, data) => {
                let values = values_or_empty(system, &key);
                match (data, find_value(&values, name)) {
                    (Data::Dword(number), Some(value)) => value.as_u32() == Some(*number),
                    (Data::Text(text) | Data::Expand(text), Some(_)) => {
                        text_value(&values, name).as_deref().unwrap_or_default() == *text
                    }
                    (_, None) => false,
                }
            }
            Change::Remove(_, name) => find_value(&values_or_empty(system, &key), name).is_none(),
            Change::RemoveKey(_) => !key_exists(system, &key),
        };
        if !holds {
            return Ok(false);
        }
    }
    Ok(true)
}

/// What the changes are about to replace, including keys they will create.
fn capture(system: &dyn System, changes: &[Change]) -> Result<Vec<Prior>> {
    let mut priors = Vec::new();
    for change in changes {
        let key = key_of(change)?;
        match change {
            Change::Set(_, name, _) => {
                if let Some(created) = first_missing(system, &key) {
                    priors.push(Prior::CreatedKey { key: created });
                }
                let old = find_value(&values_or_empty(system, &key), name).cloned();
                priors.push(Prior::Value {
                    key,
                    name: (*name).to_owned(),
                    old,
                });
            }
            Change::Remove(_, name) => {
                let old = find_value(&values_or_empty(system, &key), name).cloned();
                priors.push(Prior::Value {
                    key,
                    name: (*name).to_owned(),
                    old,
                });
            }
            Change::RemoveKey(_) => {
                let old = if key_exists(system, &key) {
                    Some(capture_key(system, &key)?)
                } else {
                    None
                };
                priors.push(Prior::Key { key, old });
            }
        }
    }
    Ok(priors)
}

/// The highest ancestor of `key` (or the key itself) that does not exist.
fn first_missing(system: &dyn System, key: &RegPath) -> Option<RegPath> {
    let mut missing = None;
    let mut current = Some(key.clone());
    while let Some(key) = current {
        if key.path.is_empty() || key_exists(system, &key) {
            break;
        }
        current = key.parent();
        missing = Some(key);
    }
    missing
}

fn apply(system: &dyn System, changes: &[Change]) -> Result<()> {
    for change in changes {
        let key = key_of(change)?;
        let result = match change {
            Change::Set(_, name, data) => system.write_value(&key, &raw(name, *data)),
            Change::Remove(_, name) => ignore_missing(system.delete_value(&key, name)),
            Change::RemoveKey(_) => ignore_missing(system.delete_key_tree(&key)),
        };
        result.map_err(|source| Error::registry("change", &key, source))?;
    }
    Ok(())
}

/// Put back what was recorded, newest change first.
fn revert(system: &dyn System, priors: &[Prior]) -> Result<()> {
    for prior in priors.iter().rev() {
        let (key, result) = match prior {
            Prior::Value {
                key,
                old: Some(value),
                ..
            } => (key, system.write_value(key, value)),
            Prior::Value {
                key,
                name,
                old: None,
            } => (key, ignore_missing(system.delete_value(key, name))),
            Prior::CreatedKey { key } => (key, ignore_missing(system.delete_key_tree(key))),
            Prior::Key {
                key,
                old: Some(tree),
            } => {
                write_tree(system, key, tree, false)?;
                continue;
            }
            Prior::Key { old: None, .. } => continue,
        };
        result.map_err(|source| Error::registry("restore", key, source))?;
    }
    Ok(())
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Windows' build number, e.g. 26100; `None` when it cannot be read.
pub fn windows_build(system: &dyn System) -> Option<u32> {
    let key = RegPath::new(
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
    );
    text_value(&values_or_empty(system, &key), "CurrentBuildNumber")?
        .parse()
        .ok()
}
