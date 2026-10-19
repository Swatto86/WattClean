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

#[test]
fn entries_for_other_windows_versions_are_left_out() {
    let text = "[Old *]\nDetectOS=|6.3\nFileKey1=%Temp%|*.old\n\
        [Newer *]\nDetectOS=6.2|\nFileKey1=%Temp%|*.new\n\
        [Ten *]\nDetectOS=10.0|10.0\nFileKey1=%Temp%|*.ten\n\
        [Unreadable *]\nDetectOS=seven|\nFileKey1=%Temp%|*.bad\n\
        [Any *]\nFileKey1=%Temp%|*.any\n";
    let names: Vec<_> = parse(text).into_iter().map(|c| c.name).collect();
    assert_eq!(names, vec!["Newer", "Ten", "Any"]);
}

#[test]
fn a_registry_detect_may_hold_wildcards() {
    let dir = tempfile::tempdir().unwrap();
    let machine = dir.path().join("machine");
    std::fs::create_dir_all(&machine).unwrap();
    std::fs::write(
        machine.join("state.json"),
        r#"{"registry": {"HKCU\\software\\office\\16.0\\word": []}}"#,
    )
    .unwrap();
    let system = crate::fake::FakeSystem::open(dir.path()).unwrap();
    let found = |key: &str| {
        let text = format!("[App *]\nDetect={key}\nFileKey1=%Temp%|*.tmp\n");
        crate::catalog::is_present(&parse(&text)[0], &system)
    };
    assert!(found(r"HKCU\Software\Office"));
    assert!(found(r"HKCU\Software\Off*\1?.0\Word"));
    assert!(found(r"HKCU\Software\Office\*"));
    assert!(!found(r"HKCU\Software\Office\1?.0\Excel"));
    assert!(!found(r"HKCU\Software\Nothing*"));
}
