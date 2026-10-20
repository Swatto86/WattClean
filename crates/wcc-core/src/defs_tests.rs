use std::collections::HashSet;
use std::time::SystemTime;

use super::*;
use crate::model::{Action, Cleaner, CleanerOption, Detect};
use crate::search::Filter;

const PLATFORMS: [Platform; 3] = [Platform::Windows, Platform::Linux, Platform::Macos];

/// The fewest built-in cleaners each platform should have.
fn fewest(platform: Platform) -> usize {
    match platform {
        Platform::Windows => 60,
        Platform::Linux => 35,
        Platform::Macos => 35,
    }
}

/// Every variable the Windows and test machines resolve.
pub const KNOWN_VARIABLES: &[&str] = &[
    "APPDATA",
    "LOCALAPPDATA",
    "LOCALLOWAPPDATA",
    "USERPROFILE",
    "TEMP",
    "TMP",
    "WINDIR",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "PROGRAMDATA",
    "COMMONAPPDATA",
    "ALLUSERSPROFILE",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "COMMONPROGRAMFILES",
    "PUBLIC",
    "DOCUMENTS",
    "DESKTOP",
    "DOWNLOADS",
    "PICTURES",
    "MUSIC",
    "VIDEO",
    "FAVORITES",
    "STARTMENU",
    "ONEDRIVE",
];

fn variables(text: &str) -> Vec<String> {
    text.split('%')
        .skip(1)
        .step_by(2)
        .map(str::to_ascii_uppercase)
        .collect()
}

fn texts(cleaner: &Cleaner) -> Vec<String> {
    let mut out = Vec::new();
    for detect in &cleaner.detect {
        match detect {
            Detect::Path(p) | Detect::Registry(p) => out.push(p.clone()),
            Detect::Program(_) => {}
        }
    }
    for option in &cleaner.options {
        for action in &option.actions {
            match action {
                Action::Delete(spec) | Action::Truncate(spec) => {
                    out.push(spec.path.clone());
                    out.extend(spec.exclude.clone());
                }
                Action::Sqlite { files, .. }
                | Action::Json { files, .. }
                | Action::Ini { files, .. } => {
                    out.push(files.path.clone());
                }
                Action::Registry { key, .. } => out.push(key.clone()),
                Action::Command(task) => {
                    out.extend(task.measure.iter().map(ToString::to_string));
                }
                _ => {}
            }
        }
    }
    out
}

#[test]
fn every_built_in_file_parses() {
    for platform in PLATFORMS {
        every_file_parses(platform);
    }
}

fn every_file_parses(platform: Platform) {
    let (cleaners, problems) = built_in(platform);
    assert!(problems.is_empty(), "{platform:?}: {problems:#?}");
    assert!(
        cleaners.len() >= fewest(platform),
        "{platform:?}: only {} cleaners",
        cleaners.len()
    );
    let mut ids = HashSet::new();
    for cleaner in &cleaners {
        assert!(
            ids.insert(cleaner.id.clone()),
            "{} is defined twice",
            cleaner.id
        );
        assert!(
            !cleaner.name.is_empty() && !cleaner.description.is_empty(),
            "{}",
            cleaner.id
        );
        for option in &cleaner.options {
            assert!(
                !option.description.is_empty(),
                "{}.{} has no description",
                cleaner.id,
                option.id
            );
        }
    }
}

#[test]
fn built_in_paths_use_only_known_variables_and_no_leftover_placeholders() {
    for platform in PLATFORMS {
        let known = match platform {
            Platform::Windows => KNOWN_VARIABLES.to_vec(),
            Platform::Linux => crate::vars::UNIX_VARIABLES.to_vec(),
            Platform::Macos => [crate::vars::UNIX_VARIABLES, crate::vars::MACOS_VARIABLES].concat(),
        };
        let (cleaners, _) = built_in(platform);
        for cleaner in &cleaners {
            for text in texts(cleaner) {
                assert!(
                    !text.contains('{'),
                    "{}: unfilled placeholder in {text}",
                    cleaner.id
                );
                for name in variables(&text) {
                    assert!(
                        known.contains(&name.as_str()),
                        "{platform:?} {}: unknown variable %{name}% in {text}",
                        cleaner.id
                    );
                }
            }
        }
    }
}

/// Registry actions exist only on Windows; commands only elsewhere, and
/// every Linux or macOS path is a full path from a variable or the root.
#[test]
fn each_platform_uses_only_its_own_kinds_of_action() {
    for platform in PLATFORMS {
        let (cleaners, _) = built_in(platform);
        for cleaner in &cleaners {
            for option in &cleaner.options {
                for action in &option.actions {
                    let key = format!("{platform:?} {}.{}", cleaner.id, option.id);
                    match action {
                        Action::Registry { .. } => {
                            assert_eq!(platform, Platform::Windows, "{key}");
                        }
                        Action::Command(_) => {
                            assert_ne!(platform, Platform::Windows, "{key}");
                        }
                        _ => {}
                    }
                }
            }
            if platform != Platform::Windows {
                for text in texts(cleaner) {
                    assert!(
                        text.starts_with('%') || text.starts_with('/'),
                        "{platform:?} {}: {text} is not a full path",
                        cleaner.id
                    );
                    assert!(!text.contains(':'), "{}: {text}", cleaner.id);
                }
            }
        }
    }
}

#[test]
fn built_in_filters_compile() {
    for platform in PLATFORMS {
        filters_compile(platform);
    }
}

fn filters_compile(platform: Platform) {
    let (cleaners, _) = built_in(platform);
    let vars = |_: &str| vec![r"C:\X".to_string()];
    for cleaner in &cleaners {
        for option in &cleaner.options {
            for action in &option.actions {
                if let Action::Delete(spec) | Action::Truncate(spec) = action {
                    assert!(
                        Filter::new(spec, &vars, SystemTime::now()).is_ok(),
                        "{}.{}",
                        cleaner.id,
                        option.id
                    );
                }
            }
        }
    }
}

#[test]
fn templates_fill_placeholders_and_skip_options() {
    let text = r#"
[[template]]
id = "t"
vars = { profile = '{root}\*' }
[[template.option]]
id = "cache"
name = "Cache"
description = "c"
actions = [{ op = "delete", search = "walk.all", path = '{profile}\Cache' }]
[[template.option]]
id = "logs"
name = "Logs"
description = "l"
actions = [{ op = "delete", search = "walk.files", path = '{root}\logs' }]

[[cleaner]]
id = "app"
name = "App"
description = "An app"
category = "applications"
template = "t"
skip = ["logs"]
vars = { root = '%APPDATA%\App' }
detect = ['{root}']
[[cleaner.option]]
id = "mru"
name = "Recent"
actions = [{ op = "registry", key = 'HKCU\Software\App', value = "MRU" }]
"#;
    let (cleaners, problems) = parse_files(&[("t.toml", text)], Source::Custom);
    assert!(problems.is_empty(), "{problems:?}");
    let app = &cleaners[0];
    assert_eq!(app.detect, vec![Detect::Path(r"%APPDATA%\App".into())]);
    let ids: Vec<_> = app.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, vec!["cache", "mru"]);
    let Action::Delete(spec) = &app.options[0].actions[0] else {
        panic!()
    };
    assert_eq!(spec.path, r"%APPDATA%\App\*\Cache");
}

#[test]
fn mistakes_are_reported_with_the_file_name() {
    let unknown_field =
        "[[cleaner]]\nid = \"a\"\nname = \"A\"\ncategory = \"system\"\ncolour = \"red\"\n";
    let (_, problems) = parse_files(&[("bad.toml", unknown_field)], Source::Custom);
    assert!(
        problems[0].to_string().starts_with("bad.toml:"),
        "{}",
        problems[0]
    );

    let no_path = r#"
[[cleaner]]
id = "a"
name = "A"
category = "system"
[[cleaner.option]]
id = "x"
name = "X"
actions = [{ op = "delete" }]
"#;
    let (cleaners, problems) = parse_files(&[("nopath.toml", no_path)], Source::Custom);
    assert!(cleaners.is_empty());
    assert!(
        problems[0].to_string().contains("needs a path"),
        "{}",
        problems[0]
    );

    let dotted = "[[cleaner]]\nid = \"a.b\"\nname = \"A\"\ncategory = \"system\"\n";
    let (_, problems) = parse_files(&[("dot.toml", dotted)], Source::Custom);
    assert!(
        problems[0].to_string().contains("must be letters"),
        "{}",
        problems[0]
    );
}

#[test]
fn find_dirs_needs_names_and_beside_needs_find_dirs() {
    let action = |a: &str| {
        format!(
            "[[cleaner]]\nid = \"a\"\nname = \"A\"\ncategory = \"developer\"\n\
             [[cleaner.option]]\nid = \"x\"\nname = \"X\"\nactions = [{a}]\n"
        )
    };
    let cases = [
        (
            r#"{ op = "delete", search = "find.dirs", path = '%USERPROFILE%' }"#,
            "needs the names",
        ),
        (
            r#"{ op = "delete", search = "walk.top", path = '%USERPROFILE%', beside = 'Cargo.toml' }"#,
            "only with find.dirs",
        ),
    ];
    for (text, expected) in cases {
        let (cleaners, problems) = parse_files(&[("f.toml", &action(text))], Source::Custom);
        assert!(cleaners.is_empty(), "{text}");
        assert!(
            problems[0].to_string().contains(expected),
            "{}",
            problems[0]
        );
    }
}

fn option_of<'a>(cleaners: &'a [Cleaner], cleaner: &str, option: &str) -> &'a CleanerOption {
    cleaners
        .iter()
        .find(|c| c.id == cleaner)
        .and_then(|c| c.options.iter().find(|o| o.id == option))
        .unwrap_or_else(|| panic!("{cleaner}.{option} is missing"))
}

#[test]
fn profile_wide_deep_scans_stay_out_of_onedrive() {
    let (cleaners, _) = built_in(Platform::Windows);
    let scan = cleaners.iter().find(|c| c.id == "deep_scan").unwrap();
    for option in &scan.options {
        for action in &option.actions {
            let Action::Delete(spec) = action else {
                continue;
            };
            if spec.path == "%USERPROFILE%" {
                assert!(
                    spec.exclude.iter().any(|e| e == "%ONEDRIVE%"),
                    "deep_scan.{} walks into OneDrive",
                    option.id
                );
            }
        }
    }
}

#[test]
fn options_that_lose_pinned_or_unsynced_things_say_so() {
    let (cleaners, _) = built_in(Platform::Windows);
    let warns = |c, o, word: &str| {
        option_of(&cleaners, c, o)
            .warning
            .as_deref()
            .is_some_and(|w| w.to_lowercase().contains(word))
    };
    assert!(warns("explorer", "recent", "pinned"));
    assert!(warns("office", "cache", "upload"));
}

#[test]
fn site_storage_leaves_browser_extension_data_alone() {
    let mut checked = 0;
    let all: Vec<Cleaner> = PLATFORMS
        .iter()
        .flat_map(|platform| built_in(*platform).0)
        .collect();
    for cleaner in &all {
        for option in cleaner.options.iter().filter(|o| o.id == "site_storage") {
            for action in &option.actions {
                let Action::Delete(spec) = action else {
                    continue;
                };
                let marker = if spec.path.ends_with(r"\storage\default") {
                    "moz-extension+++"
                } else if spec.path.ends_with(r"\IndexedDB") {
                    "chrome-extension_"
                } else {
                    continue;
                };
                checked += 1;
                assert!(
                    spec.exclude.iter().any(|e| e.contains(marker)),
                    "{}.{} deletes extension data",
                    cleaner.id,
                    option.id
                );
            }
        }
    }
    assert!(checked >= 2, "only {checked} storage actions found");
}
