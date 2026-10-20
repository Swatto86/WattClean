use super::*;

// Windows paths: a Linux or macOS build rightly refuses them.
#[cfg(windows)]
#[test]
fn loading_keeps_every_field_that_parses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"theme":"dark","shred":"yes","accent":"mauve","keep":[{"path":"C:\\Keep","folder":true}],"future":1}"#,
    )
    .unwrap();
    let settings = Settings::load(&path);
    assert_eq!(settings.theme, Theme::Dark);
    assert!(!settings.shred, "a bad value keeps the default");
    assert_eq!(settings.accent, "sky");
    assert_eq!(settings.keep.len(), 1);
}

#[cfg(unix)]
#[test]
fn unix_paths_are_full_paths_and_drive_paths_are_not() {
    let mut settings = Settings::default();
    let entry = |p: &str| CustomPath {
        path: p.into(),
        folder: false,
    };
    settings
        .apply(SettingsPatch {
            keep: Some(vec![entry(" /home/t/b.txt "), entry("/home/t/b.txt")]),
            ..SettingsPatch::default()
        })
        .unwrap();
    assert_eq!(settings.keep, vec![entry("/home/t/b.txt")]);
    let drive = SettingsPatch {
        keep: Some(vec![entry(r"C:\A")]),
        ..SettingsPatch::default()
    };
    assert!(settings.apply(drive).is_err());
}

#[test]
fn patches_are_validated() {
    let mut settings = Settings::default();
    let relative = SettingsPatch {
        keep: Some(vec![CustomPath {
            path: r"relative\folder".into(),
            folder: true,
        }]),
        ..SettingsPatch::default()
    };
    assert!(settings.apply(relative).is_err());
    let variable = SettingsPatch {
        custom_paths: Some(vec![CustomPath {
            path: r"%TEMP%\x".into(),
            folder: false,
        }]),
        ..SettingsPatch::default()
    };
    assert!(settings.apply(variable).is_err());
    let selection = SettingsPatch {
        selection: Some(vec!["nodot".into()]),
        ..SettingsPatch::default()
    };
    assert!(settings.apply(selection).is_err());
    assert!(serde_json::from_str::<SettingsPatch>(r#"{"unknown":true}"#).is_err());
}

// Windows paths: a Linux or macOS build rightly refuses them.
#[cfg(windows)]
#[test]
fn keep_entries_cannot_hold_wildcards() {
    let wildcard = SettingsPatch {
        keep: Some(vec![CustomPath {
            path: r"C:\Docs\*.docx".into(),
            folder: false,
        }]),
        ..SettingsPatch::default()
    };
    let error = Settings::default().apply(wildcard).unwrap_err();
    assert!(error.contains("wildcard"), "{error}");

    // An old file with one such entry keeps the others.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"keep":[{"path":"C:\\A","folder":true},{"path":"C:\\B\\?.txt","folder":false}]}"#,
    )
    .unwrap();
    let loaded = Settings::load(&path);
    assert_eq!(loaded.keep.len(), 1);
    assert_eq!(loaded.keep[0].path, r"C:\A");
}

// Windows paths: a Linux or macOS build rightly refuses them.
#[cfg(windows)]
#[test]
fn lists_are_trimmed_and_deduplicated() {
    let mut settings = Settings::default();
    let entry = |p: &str| CustomPath {
        path: p.into(),
        folder: false,
    };
    settings
        .apply(SettingsPatch {
            keep: Some(vec![entry(r" C:\A\b.txt "), entry(r"c:\a\B.TXT")]),
            ..SettingsPatch::default()
        })
        .unwrap();
    assert_eq!(settings.keep, vec![entry(r"C:\A\b.txt")]);
}

#[test]
fn the_selection_falls_back_to_defaults() {
    let defaults = vec![OptionKey::new("a", "b")];
    let mut settings = Settings::default();
    assert_eq!(settings.selected(&defaults), defaults);
    settings.selection = Some(vec!["c.d".into()]);
    assert_eq!(settings.selected(&defaults), vec![OptionKey::new("c", "d")]);
}

#[test]
fn a_chosen_path_may_hold_a_percent_sign() {
    let full = if cfg!(windows) {
        r"C:\Users\T\100% done.pdf"
    } else {
        "/home/t/100% done.pdf"
    };
    assert_eq!(check_full_path(full), Ok(()));
    let named = if cfg!(windows) {
        r"C:\Users\T\%TEMP%.txt"
    } else {
        "/home/t/%TEMP%.txt"
    };
    assert_eq!(check_full_path(named), Ok(()), "a name, not a variable");
    let error = check_full_path(r"relative\file.txt").unwrap_err();
    let example = if cfg!(windows) {
        r"C:\Folder"
    } else if cfg!(target_os = "macos") {
        "/Users/you/Folder"
    } else {
        "/home/you/Folder"
    };
    assert!(error.ends_with(&format!("such as {example}")), "{error}");
    assert!(check_full_path(" /home/t/a.txt").is_err(), "taken as given");
}

/// The guard folds `..` by the text of a path, the system by following
/// links, so `Link/..` would reach a folder the guard never looked at.
#[test]
fn a_chosen_path_never_steps_back_up() {
    let back = if cfg!(windows) {
        r"C:\Users\T\Link\..\Documents"
    } else {
        "/home/t/Link/../Documents"
    };
    let error = check_full_path(back).unwrap_err();
    assert!(error.contains(".."), "{error}");
    assert!(check_path(back).is_err(), "the lists too");
    let dotted = if cfg!(windows) {
        r"C:\Users\T\..notes"
    } else {
        "/home/t/..notes"
    };
    assert_eq!(check_full_path(dotted), Ok(()), "a name, not a step back");
}

#[cfg(unix)]
#[test]
fn lists_refuse_variables_but_not_percent_signs() {
    let custom = |p: &str| SettingsPatch {
        custom_paths: Some(vec![CustomPath {
            path: p.into(),
            folder: true,
        }]),
        ..SettingsPatch::default()
    };
    let mut settings = Settings::default();
    settings.apply(custom("/home/t/100% done")).unwrap();
    let error = settings.apply(custom("/home/t/%TEMP%")).unwrap_err();
    assert!(error.contains("variable"), "{error}");
}
