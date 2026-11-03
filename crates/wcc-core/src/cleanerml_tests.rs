
use super::*;

const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<cleaner id="example" os="windows">
  <label>Example</label>
  <description>An example program</description>
  <running type="exe">Example.exe</running>
  <var name="base">
    <value>$APPDATA\Example</value>
    <value>${LOCALAPPDATA}\Example</value>
  </var>
  <option id="cache">
    <label>Cache</label>
    <description>Delete the cache</description>
    <action command="delete" search="walk.files" path="$$base$$\Cache" nregex="\.keep$"/>
    <action command="delete" search="file" path="~/.example" os="linux"/>
  </option>
  <option id="mru">
    <label>Recent files</label>
    <warning>Your recent list goes.</warning>
    <action command="winreg" path="HKCU\Software\Example" name="MRU"/>
    <action command="ini" search="file" path="%APPDATA%\Example\ex.ini" section="Recent"/>
    <action command="chrome.history" search="file" path="%APPDATA%\Example\History"/>
    <action command="teleport" search="file" path="x"/>
  </option>
  <option id="linux" os="linux">
    <label>Linux only</label>
    <action command="delete" search="file" path="~/.x"/>
  </option>
</cleaner>"#;

#[test]
fn reads_options_actions_and_variables() {
    let (cleaner, notes) = parse(SAMPLE, Platform::Windows).unwrap();
    assert_eq!(cleaner.id, "ml-example");
    assert_eq!(cleaner.name, "Example");
    assert_eq!(cleaner.running, vec!["example.exe".to_string()]);
    assert_eq!(cleaner.options.len(), 2);
    let cache = &cleaner.options[0];
    let paths: Vec<_> = cache
        .actions
        .iter()
        .map(|a| match a {
            Action::Delete(spec) => spec.path.clone(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        paths,
        vec![
            r"%APPDATA%\Example\Cache".to_string(),
            r"%LOCALAPPDATA%\Example\Cache".to_string()
        ]
    );
    let mru = &cleaner.options[1];
    assert_eq!(mru.warning.as_deref(), Some("Your recent list goes."));
    assert_eq!(
        mru.actions[0],
        Action::Registry {
            key: r"HKCU\Software\Example".into(),
            value: Some("MRU".into())
        }
    );
    assert!(matches!(
        mru.actions[2],
        Action::Sqlite {
            op: SqliteOp::ChromeHistory,
            ..
        }
    ));
    assert_eq!(
        notes,
        vec![r#"mru: command "teleport" is not supported"#.to_string()]
    );
}

#[test]
fn refuses_what_is_not_a_windows_cleaner() {
    assert!(parse("<nope/>", Platform::Windows).is_err());
    let linux = r#"<cleaner id="x" os="linux"><option id="a"/></cleaner>"#;
    assert!(parse(linux, Platform::Windows).is_err());
    assert!(parse("not xml", Platform::Windows).is_err());
}

/// BleachBit's own cleaners are mostly Linux ones.
#[test]
fn linux_reads_its_own_parts_with_the_home_folder() {
    let text = r#"<cleaner id="gedit">
  <label>gedit</label>
  <option id="cache">
    <label>Cache</label>
    <action command="delete" search="walk.all" path="~/.cache/gedit"/>
    <action command="delete" search="file" path="$XDG_CONFIG_HOME/gedit/recent" os="linux"/>
    <action command="delete" search="file" path="$APPDATA\gedit\cache" os="windows"/>
    <action command="winreg" path="HKCU\Software\gedit"/>
  </option>
</cleaner>"#;
    let (cleaner, notes) = parse(text, Platform::Linux).unwrap();
    let paths: Vec<_> = cleaner.options[0]
        .actions
        .iter()
        .filter_map(|a| match a {
            Action::Delete(spec) => Some(spec.path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        paths,
        ["%HOME%/.cache/gedit", "%XDG_CONFIG_HOME%/gedit/recent"]
    );
    assert_eq!(notes, ["cache: registry actions apply to Windows only"]);
    let (windows, _) = parse(text, Platform::Windows).unwrap();
    assert_eq!(windows.options[0].actions.len(), 3, "~ stays literal there");
    let Action::Delete(spec) = &windows.options[0].actions[0] else {
        panic!()
    };
    assert_eq!(spec.path, "~/.cache/gedit");
}

#[test]
fn winreg_paths_use_the_cleaners_variables() {
    let text = r#"<cleaner id="x">
  <var name="hive"><value>$$base$$\Recent</value></var>
  <var name="base"><value>HKCU\Software\Example</value></var>
  <option id="a"><action command="winreg" path="$$hive$$"/></option>
</cleaner>"#;
    let (cleaner, notes) = parse(text, Platform::Windows).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        cleaner.options[0].actions,
        [Action::Registry {
            key: r"HKCU\Software\Example\Recent".into(),
            value: None,
        }]
    );
}
