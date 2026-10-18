use std::collections::HashSet;
use std::time::SystemTime;

use super::*;
use crate::model::{Action, Detect};
use crate::search::Filter;

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
                _ => {}
            }
        }
    }
    out
}

#[test]
fn every_built_in_file_parses() {
    let (cleaners, problems) = built_in();
    assert!(problems.is_empty(), "{problems:#?}");
    assert!(cleaners.len() >= 60, "only {} cleaners", cleaners.len());
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
    let (cleaners, _) = built_in();
    for cleaner in &cleaners {
        for text in texts(cleaner) {
            assert!(
                !text.contains('{'),
                "{}: unfilled placeholder in {text}",
                cleaner.id
            );
            for name in variables(&text) {
                assert!(
                    KNOWN_VARIABLES.contains(&name.as_str()),
                    "{}: unknown variable %{name}% in {text}",
                    cleaner.id
                );
            }
        }
    }
}

#[test]
fn built_in_filters_compile() {
    let (cleaners, _) = built_in();
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
