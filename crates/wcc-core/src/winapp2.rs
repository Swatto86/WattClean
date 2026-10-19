//! winapp2.ini, the community list of extra cleaners that BleachBit and
//! CCleaner both read. Each `[Entry *]` becomes one cleaner with one option.
//!
//! Supported keys: `Detect*` (wildcards allowed), `DetectFile*`,
//! `DetectOS` (an entry for other Windows versions is left out),
//! `SpecialDetect`, `FileKey*`
//! (with `RECURSE` and `REMOVESELF`), `RegKey*`, `ExcludeKey*` (`FILE` and
//! `PATH`), `Warning`, `LangSecRef` and `Section`. Anything else is ignored.

use std::collections::HashSet;

use crate::model::{
    Action, Category, Cleaner, CleanerOption, Detect, FileSpec, Kind, Search, Source, slug,
};
use crate::system::split_hive;

/// Detection CCleaner does in code, mapped to the folders it looks for.
fn special_detect(name: &str) -> Option<&'static str> {
    Some(match name.trim().to_ascii_uppercase().as_str() {
        "DET_CHROME" => r"%LocalAppData%\Google\Chrome\User Data",
        "DET_MOZILLA" => r"%AppData%\Mozilla\Firefox",
        "DET_THUNDERBIRD" => r"%AppData%\Thunderbird",
        "DET_OPERA" => r"%AppData%\Opera Software",
        "DET_EDGE" => r"%LocalAppData%\Microsoft\Edge\User Data",
        "DET_WINSTORE" => r"%LocalAppData%\Packages",
        _ => return None,
    })
}

fn group(lang_sec_ref: &str) -> &'static str {
    match lang_sec_ref.trim() {
        "3001" => "Internet Explorer",
        "3005" => "Microsoft Edge",
        "3006" => "Windows Explorer",
        "3021" => "Applications",
        "3022" => "Internet",
        "3023" => "Multimedia",
        "3024" => "Utilities",
        "3025" => "Windows",
        "3026" => "Firefox",
        "3027" => "Opera",
        "3028" => "Safari",
        "3029" => "Google Chrome",
        "3030" => "Thunderbird",
        "3031" => "Windows Store",
        _ => "Community",
    }
}

#[derive(Default)]
struct Entry {
    name: String,
    group: String,
    warning: Option<String>,
    detect: Vec<Detect>,
    files: Vec<String>,
    registry: Vec<String>,
    exclude: Vec<String>,
    /// `DetectOS` names other Windows versions (or cannot be read).
    other_windows: bool,
}

/// What every Windows WattClean runs on reports: Windows 10 and 11 are both
/// 10.0, and Rust's own programs need Windows 10.
const WINDOWS: [u32; 2] = [10, 0];

/// `DetectOS=min|max`, either side empty for no bound: whether this
/// Windows is in the range. A range that cannot be read does not fit.
fn fits_this_windows(range: &str) -> bool {
    let version = |text: &str| -> Option<Option<Vec<u32>>> {
        let text = text.trim();
        if text.is_empty() {
            return Some(None);
        }
        let mut parts: Vec<u32> = text
            .split('.')
            .map(|part| part.trim().parse().ok())
            .collect::<Option<_>>()?;
        while parts.len() < WINDOWS.len() {
            parts.push(0);
        }
        Some(Some(parts))
    };
    let Some((min, max)) = range.split_once('|') else {
        return false;
    };
    let (Some(min), Some(max)) = (version(min), version(max)) else {
        return false;
    };
    min.is_none_or(|min| WINDOWS.as_slice() >= min.as_slice())
        && max.is_none_or(|max| WINDOWS.as_slice() <= max.as_slice())
}

/// Every entry in the file; entries with nothing to clean are left out.
pub fn parse(text: &str) -> Vec<Cleaner> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut current: Option<Entry> = None;
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            entries.extend(current.take());
            current = Some(Entry {
                name: name.trim().trim_end_matches('*').trim().to_string(),
                group: "Community".to_string(),
                ..Entry::default()
            });
            continue;
        }
        let (Some(entry), Some((key, value))) = (current.as_mut(), line.split_once('=')) else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        let base = key.trim_end_matches(|c: char| c.is_ascii_digit());
        match base {
            "detect" => entry.detect.push(Detect::Registry(value)),
            "detectfile" => entry.detect.push(Detect::Path(value)),
            "detectos" => entry.other_windows |= !fits_this_windows(&value),
            "specialdetect" => {
                if let Some(path) = special_detect(&value) {
                    entry.detect.push(Detect::Path(path.to_string()));
                }
            }
            "filekey" => entry.files.push(value),
            "regkey" => entry.registry.push(value),
            "excludekey" => entry.exclude.push(value),
            "warning" => entry.warning = Some(value),
            "langsecref" => entry.group = group(&value).to_string(),
            "section" => entry.group = value,
            _ => {}
        }
    }
    entries.extend(current);

    let mut ids = HashSet::new();
    entries
        .into_iter()
        .filter_map(|entry| build(entry, &mut ids))
        .collect()
}

fn excludes(list: &[String]) -> Vec<String> {
    list.iter()
        .filter_map(|line| {
            let mut parts = line.split('|');
            let kind = parts.next()?.trim().to_ascii_uppercase();
            let path = parts.next()?.trim().trim_end_matches('\\');
            let pattern = parts.next().map(str::trim).unwrap_or("*.*");
            match kind.as_str() {
                "PATH" if pattern == "*.*" || pattern == "*" => Some(path.to_string()),
                "PATH" | "FILE" => Some(format!(r"{path}\{pattern}")),
                _ => None,
            }
        })
        .collect()
}

/// `hive\path` in one comparable form: short hive, lower case, no trailing `\`.
fn reg_key(key: &str) -> String {
    let key = key.trim().trim_end_matches('\\');
    match split_hive(key) {
        Some((hive, "")) => hive.to_ascii_lowercase(),
        Some((hive, rest)) => format!(r"{hive}\{rest}").to_ascii_lowercase(),
        None => key.to_ascii_lowercase(),
    }
}

/// Registry keys and values an entry says to leave alone: (key, value).
fn reg_excludes(list: &[String]) -> Vec<(String, Option<String>)> {
    list.iter()
        .filter_map(|line| {
            let mut parts = line.split('|');
            if !parts.next()?.trim().eq_ignore_ascii_case("REG") {
                return None;
            }
            let key = reg_key(parts.next()?);
            let value = parts.next().map(|v| v.trim().to_ascii_lowercase());
            Some((key, value.filter(|v| !v.is_empty())))
        })
        .collect()
}

/// Whether deleting `key` (or its `value`) would take something an
/// ExcludeKey protects: the exclusion is that key or inside it.
fn removes_excluded(key: &str, value: Option<&str>, excluded: &[(String, Option<String>)]) -> bool {
    let key = reg_key(key);
    excluded.iter().any(|(ex_key, ex_value)| match value {
        None => ex_key == &key || ex_key.starts_with(&format!("{key}\\")),
        Some(name) => {
            ex_key == &key
                && ex_value
                    .as_deref()
                    .is_none_or(|v| v == name.to_ascii_lowercase())
        }
    })
}

fn file_actions(line: &str, exclude: &[String]) -> Vec<Action> {
    let mut parts = line.split('|');
    let path = parts.next().unwrap_or("").trim().trim_end_matches('\\');
    if path.is_empty() {
        return Vec::new();
    }
    let patterns = parts
        .next()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("*.*");
    let flag = parts
        .next()
        .map(|f| f.trim().to_ascii_uppercase())
        .unwrap_or_default();
    let spec = |path: String, search: Search, names: Option<String>| {
        let mut spec = FileSpec::new(path, search);
        spec.names = names;
        spec.exclude = exclude.to_vec();
        spec
    };
    match flag.as_str() {
        "RECURSE" => vec![Action::Delete(spec(
            path.to_string(),
            Search::WalkFiles,
            Some(patterns.to_string()),
        ))],
        "REMOVESELF" => vec![Action::Delete(spec(
            path.to_string(),
            Search::WalkTop,
            Some(patterns.to_string()),
        ))],
        _ => patterns
            .split(';')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|pattern| {
                let mut s = spec(format!(r"{path}\{pattern}"), Search::Glob, None);
                s.kind = Some(Kind::File);
                Action::Delete(s)
            })
            .collect(),
    }
}

fn build(entry: Entry, ids: &mut HashSet<String>) -> Option<Cleaner> {
    let exclude = excludes(&entry.exclude);
    let reg_exclude = reg_excludes(&entry.exclude);
    let mut actions: Vec<Action> = entry
        .files
        .iter()
        .flat_map(|f| file_actions(f, &exclude))
        .collect();
    for line in &entry.registry {
        let mut parts = line.split('|');
        let key = parts.next().unwrap_or("").trim().to_string();
        if key.is_empty() {
            continue;
        }
        let value = parts
            .next()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        // A REG exclusion cannot carve a hole in a key delete, so the whole
        // action is dropped (the safe side).
        if removes_excluded(&key, value.as_deref(), &reg_exclude) {
            continue;
        }
        actions.push(Action::Registry { key, value });
    }
    if actions.is_empty() || entry.name.is_empty() || entry.other_windows {
        return None;
    }
    let base = format!("w2-{}", slug(&entry.name));
    let mut id = base.clone();
    let mut n = 2;
    while !ids.insert(id.clone()) {
        id = format!("{base}-{n}");
        n += 1;
    }
    Some(Cleaner {
        id,
        name: entry.name,
        description: format!("winapp2.ini · {}", entry.group),
        category: Category::Community,
        source: Source::Community,
        running: Vec::new(),
        detect: entry.detect,
        options: vec![CleanerOption {
            id: "clean".to_string(),
            name: "Clean".to_string(),
            description: "Everything this community entry lists".to_string(),
            warning: entry.warning,
            default: false,
            actions,
        }],
    })
}

#[cfg(test)]
#[path = "winapp2_tests.rs"]
mod tests;
