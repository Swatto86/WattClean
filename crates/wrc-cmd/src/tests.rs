//! The command line on the fake machine: what each command reads, changes
//! and says.

use super::*;
use wrc_core::RawValue;
use wrc_core::fake::FakeSystem;
use wrc_core::model::Hive;

fn args(text: &[&str]) -> Vec<String> {
    text.iter().map(|arg| (*arg).to_owned()).collect()
}

fn run_on(system: &FakeSystem, dir: &Path, running: bool, text: &[&str]) -> (i32, String) {
    let command = match parse(&args(text)) {
        Ok(command) => command,
        Err(error) => return (2, error),
    };
    let machine = Machine {
        system,
        dir,
        app_running: running,
    };
    let mut out = Vec::new();
    let code = run(&command, &machine, &mut out);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn arguments_read_like_revos() {
    assert_eq!(parse(&[]), Ok(Command::Help));
    assert_eq!(
        parse(&args(&["-clean", "-advanced", "-all"])),
        Ok(Command::Clean {
            mode: Mode::Advanced,
            all: true
        })
    );
    assert_eq!(
        parse(&args(&["/SCAN", "/Safe"])),
        Ok(Command::Scan {
            mode: Mode::Safe,
            all: false
        })
    );
    assert_eq!(
        parse(&args(&["-watch", "-export", "-all", "-txt", r"C:\Logs"])),
        Ok(Command::WatchExport {
            key: None,
            format: Format::Text,
            folder: PathBuf::from(r"C:\Logs"),
        })
    );
    assert_eq!(parse(&args(&["-backup", "-full"])), Ok(Command::BackupFull));
    assert!(parse(&args(&["-clean"])).is_err(), "a mode is needed");
    assert!(parse(&args(&["-backup", "-key", r"HKCR\.txt"])).is_err());
    assert!(
        parse(&args(&["-watch", "-start", "HKCU"])).is_err(),
        "not a hive"
    );
    assert!(parse(&args(&["-defrag"])).is_err());
}

#[test]
fn scan_lists_what_clean_then_removes_with_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(&dir.path().join("machine")).unwrap();
    let run_key = RegPath::new(
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Run",
    );
    let before = system.values_of(&run_key);
    let (code, text) = run_on(&system, dir.path(), false, &["-scan", "-safe"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("FabrikamUpdater"), "{text}");
    assert!(text.contains("would be removed by -clean"), "{text}");
    assert_eq!(system.values_of(&run_key), before, "a scan changes nothing");

    let (code, text) = run_on(&system, dir.path(), false, &["-clean", "-safe"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Removed"), "{text}");
    assert!(text.contains("Backup:"), "{text}");
    assert!(
        !system
            .values_of(&run_key)
            .iter()
            .any(|value| value.name == "FabrikamUpdater")
    );
    let backups = wrc_core::backup::list(&dir.path().join("backups")).unwrap();
    assert_eq!(backups[0].label, "Command line clean");
}

#[test]
fn backups_are_listed_and_restored_but_a_full_one_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(&dir.path().join("machine")).unwrap();
    let run_key = RegPath::new(
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Run",
    );
    assert_eq!(
        run_on(&system, dir.path(), false, &["-clean", "-safe"]).0,
        0
    );
    let (code, list) = run_on(&system, dir.path(), false, &["-backups"]);
    assert_eq!(code, 0, "{list}");
    assert!(list.contains("Command line clean"), "{list}");
    let id = list.split_whitespace().next().unwrap().to_owned();
    let (code, text) = run_on(&system, dir.path(), false, &["-restore", &id]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("entries restored"), "{text}");
    assert!(
        system
            .values_of(&run_key)
            .iter()
            .any(|value| value.name == "FabrikamUpdater"),
        "the cleaned entry is back"
    );
    let (code, _) = run_on(&system, dir.path(), false, &["-backup", "-full"]);
    assert_eq!(code, 0);
    let (_, list) = run_on(&system, dir.path(), false, &["-backups"]);
    let full = list.lines().find(|line| line.contains("Full")).unwrap();
    let id = full.split_whitespace().next().unwrap().to_owned();
    let (code, text) = run_on(&system, dir.path(), false, &["-restore", &id]);
    assert_eq!(code, 2);
    assert!(
        text.contains("does not put a whole registry back"),
        "{text}"
    );
    assert!(
        parse(&args(&["-restore"])).is_err(),
        "a backup must be named"
    );
}

#[test]
fn a_key_is_backed_up() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::empty(dir.path());
    let key = RegPath::new(Hive::CurrentUser, r"Software\Acme");
    system.put(&key, vec![RawValue::string("a", "1")]);
    let (code, text) = run_on(
        &system,
        dir.path(),
        false,
        &["-backup", "-key", r"HKEY_CURRENT_USER\Software\Acme"],
    );
    assert_eq!(code, 0, "{text}");
    assert!(
        text.starts_with(r"Backed up HKCU\Software\Acme as backup "),
        "{text}"
    );
    let (code, _) = run_on(
        &system,
        dir.path(),
        false,
        &["-backup", "-key", r"HKCU\Software\Missing"],
    );
    assert_eq!(code, 2, "a missing key cannot be backed up");
}

#[test]
fn the_watch_list_changes_only_while_the_app_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::empty(dir.path());
    let key = RegPath::new(Hive::CurrentUser, r"Software\Acme");
    system.put(&key, vec![]);
    let typed = r"HKCU\Software\Acme";
    let (code, text) = run_on(&system, dir.path(), true, &["-watch", "-start", typed]);
    assert_eq!(code, 2);
    assert!(text.contains("WattClean is open"), "{text}");

    assert_eq!(
        run_on(&system, dir.path(), false, &["-watch", "-start", typed]).0,
        0
    );
    assert!(!offline::saved(dir.path()).unwrap()[0].paused);
    assert_eq!(
        run_on(&system, dir.path(), false, &["-watch", "-stop", typed]).0,
        0
    );
    assert!(offline::saved(dir.path()).unwrap()[0].paused);
    assert_eq!(
        run_on(&system, dir.path(), false, &["-watch", "-start", typed]).0,
        0,
        "start resumes a paused watch"
    );
    assert_eq!(
        run_on(&system, dir.path(), false, &["-watch", "-del", typed]).0,
        0
    );
    assert!(offline::saved(dir.path()).unwrap().is_empty());
    let (code, text) = run_on(&system, dir.path(), false, &["-watch", "-del", typed]);
    assert_eq!(code, 2);
    assert!(text.contains("is not watched"), "{text}");
}

#[test]
fn logs_export_as_csv_or_text_for_one_key_or_all() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::empty(&dir.path().join("machine"));
    for name in ["Acme", "Beta"] {
        system.put(
            &RegPath::new(Hive::CurrentUser, &format!(r"Software\{name}")),
            vec![],
        );
        let typed = format!(r"HKCU\Software\{name}");
        assert_eq!(
            run_on(&system, dir.path(), false, &["-watch", "-start", &typed]).0,
            0
        );
    }
    let out = dir.path().join("exports");
    let folder = out.to_string_lossy().into_owned();
    let (code, text) = run_on(
        &system,
        dir.path(),
        false,
        &["-watch", "-export", "-csv", r"HKCU\Software\Acme", &folder],
    );
    assert_eq!(code, 0, "{text}");
    let csv =
        std::fs::read_to_string(out.join("WattClean watch log HKCU Software Acme.csv")).unwrap();
    assert!(
        csv.starts_with("Time (ms since 1970),Time (UTC),Operation"),
        "{csv}"
    );
    let (code, text) = run_on(
        &system,
        dir.path(),
        false,
        &["-watch", "-export", "-xlsx", r"HKCU\Software\Acme", &folder],
    );
    assert_eq!(code, 0, "{text}");
    let workbook = std::fs::read(out.join("WattClean watch log HKCU Software Acme.xlsx")).unwrap();
    assert_eq!(&workbook[..2], b"PK");
    let (code, text) = run_on(
        &system,
        dir.path(),
        false,
        &["-watch", "-export", "-all", "-txt", &folder],
    );
    assert_eq!(code, 0, "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(
        out.join("WattClean watch log HKCU Software Beta.txt")
            .is_file()
    );
}
