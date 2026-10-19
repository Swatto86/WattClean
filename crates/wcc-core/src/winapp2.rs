//! winapp2.ini, the community list of extra cleaners that BleachBit and
//! CCleaner both read. Each `[Entry *]` becomes one cleaner with one option.
//!
//! Supported keys: `Detect*`, `DetectFile*`, `SpecialDetect`, `FileKey*`
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
    if actions.is_empty() || entry.name.is_empty() {
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
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}; Version: 251001
[Example App *]
LangSecRef=3021
DetectFile=%AppData%\\Example
Detect1=HKCU\\Software\\Example
Default=False
Warning=This removes your recent projects.
FileKey1=%AppData%\\Example\\Cache|*.*|RECURSE
FileKey2=%AppData%\\Example\\Logs\\|*.log;*.txt
FileKey3=%LocalAppData%\\Example\\Temp|*.*|REMOVESELF
RegKey1=HKCU\\Software\\Example\\Recent
RegKey2=HKCU\\Software\\Example|LastFile
ExcludeKey1=PATH|%AppData%\\Example\\Cache\\Keep\\|*.*
ExcludeKey2=FILE|%AppData%\\Example\\Logs\\|important.log

[Nothing To Do *]
DetectFile=%AppData%\\Nothing

[Example App *]
Section=Duplicates
FileKey1=%Temp%\\example*.tmp
";

    #[test]
    fn parses_entries_into_cleaners() {
        let cleaners = parse(SAMPLE);
        assert_eq!(
            cleaners.len(),
            2,
            "the entry with nothing to clean is left out"
        );
        let first = &cleaners[0];
        assert_eq!(first.id, "w2-example_app");
        assert_eq!(first.name, "Example App");
        assert_eq!(first.description, "winapp2.ini · Applications");
        assert_eq!(
            first.detect,
            vec![
                Detect::Path(r"%AppData%\Example".into()),
                Detect::Registry(r"HKCU\Software\Example".into())
            ]
        );
        let option = &first.options[0];
        assert_eq!(
            option.warning.as_deref(),
            Some("This removes your recent projects.")
        );
        assert_eq!(option.actions.len(), 6);
        let Action::Delete(recurse) = &option.actions[0] else {
            panic!()
        };
        assert_eq!(recurse.search, Search::WalkFiles);
        assert_eq!(recurse.names.as_deref(), Some("*.*"));
        assert_eq!(
            recurse.exclude,
            vec![
                r"%AppData%\Example\Cache\Keep".to_string(),
                r"%AppData%\Example\Logs\important.log".to_string()
            ]
        );
        let Action::Delete(glob) = &option.actions[1] else {
            panic!()
        };
        assert_eq!(glob.path, r"%AppData%\Example\Logs\*.log");
        assert_eq!(glob.search, Search::Glob);
        let Action::Delete(top) = &option.actions[3] else {
            panic!()
        };
        assert_eq!(top.search, Search::WalkTop);
        assert_eq!(
            option.actions[5],
            Action::Registry {
                key: r"HKCU\Software\Example".into(),
                value: Some("LastFile".into())
            }
        );
        assert_eq!(cleaners[1].id, "w2-example_app-2");
        assert_eq!(cleaners[1].description, "winapp2.ini · Duplicates");
    }

    #[test]
    fn a_registry_exclusion_inside_a_regkey_keeps_that_key() {
        let text = "[App *]\nFileKey1=%AppData%\\App|*.log\n\
            RegKey1=HKCU\\Software\\App\nRegKey2=HKEY_CURRENT_USER\\Software\\Other\n\
            RegKey3=HKCU\\Software\\Clear|Value\n\
            ExcludeKey1=REG|HKCU\\Software\\App\\Settings\n\
            ExcludeKey2=REG|HKCU\\Software\\other\\\n\
            ExcludeKey3=REG|HKCU\\Software\\Clear|Value\n";
        let option = &parse(text)[0].options[0];
        let keys: Vec<_> = option
            .actions
            .iter()
            .filter(|a| matches!(a, Action::Registry { .. }))
            .collect();
        assert!(keys.is_empty(), "{keys:?}");
        assert_eq!(option.actions.len(), 1, "the file action stays");
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(parse("not an ini\n=\n[]\nFileKey1=\n").is_empty());
    }
}
