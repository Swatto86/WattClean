//! Adapter tests against private registry hives and harmless processes, so
//! the machine's own registry is never touched.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use winreg::RegKey;
use wu_core::backup::{remove, restore};
use wu_core::model::{REG_BINARY, REG_MULTI_SZ, REG_QWORD, RawValue, RegPath};
use wu_core::scan::{Confidence, Leftover, Target};
use wu_core::{Hive, Launch, System};

use super::WindowsSystem;

fn hive_system(dir: &tempfile::TempDir) -> WindowsSystem {
    let machine = RegKey::load_app_key(dir.path().join("machine.hive"), false).unwrap();
    let user = RegKey::load_app_key(dir.path().join("user.hive"), false).unwrap();
    WindowsSystem::with_hives(machine, user)
}

fn values() -> Vec<RawValue> {
    vec![
        RawValue::string("", "default text"),
        RawValue::expand_string("Path", r"%ProgramFiles%\Acme"),
        RawValue::dword("Count", 7),
        RawValue {
            name: "Big".into(),
            kind: REG_QWORD,
            bytes: 9u64.to_le_bytes().to_vec(),
        },
        RawValue {
            name: "List".into(),
            kind: REG_MULTI_SZ,
            bytes: "a\0b\0\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect(),
        },
        RawValue {
            name: "Blob".into(),
            kind: REG_BINARY,
            bytes: vec![0, 1, 254, 255],
        },
    ]
}

#[test]
fn registry_values_round_trip_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let system = hive_system(&dir);
    let key = RegPath::new(Hive::LocalMachine, r"SOFTWARE\Acme\Widget");
    for value in values() {
        system.write_value(&key, &value).unwrap();
    }
    system.create_key(&key.child("Child")).unwrap();
    let mut read = system.values(&key).unwrap();
    read.sort_by(|a, b| a.name.cmp(&b.name));
    let mut want = values();
    want.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(read, want);
    assert_eq!(system.subkeys(&key).unwrap(), vec!["Child".to_owned()]);
    assert!(system.key_modified(&key).unwrap() > 1_700_000_000);

    system.delete_value(&key, "Blob").unwrap();
    assert_eq!(system.values(&key).unwrap().len(), want.len() - 1);
    system.delete_key_tree(&key).unwrap();
    let missing = system.subkeys(&key).unwrap_err();
    assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
    assert!(
        system
            .subkeys(&RegPath::new(Hive::LocalMachine, "SOFTWARE\\Acme"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_removed_key_is_restored_exactly_through_the_backup() {
    let dir = tempfile::tempdir().unwrap();
    let system = hive_system(&dir);
    let key = RegPath::new(Hive::CurrentUser, r"Software\Acme\Widget");
    for value in values() {
        system.write_value(&key.child(r"Deep\Er"), &value).unwrap();
        system.write_value(&key, &value).unwrap();
    }
    let leftover = Leftover {
        id: 1,
        target: Target::Key { key: key.clone() },
        reason: String::new(),
        confidence: Confidence::High,
        selected: true,
        bytes: 0,
    };
    let backups = dir.path().join("backups");
    let removal = remove(&system, &backups, "Acme Widget", &[leftover], 100).unwrap();
    assert_eq!(removal.removed.keys, 1, "{:?}", removal.failed);
    assert!(system.subkeys(&key).is_err());
    restore(&system, &backups, &removal.backup.id, 200).unwrap();
    let mut back = system.values(&key.child(r"Deep\Er")).unwrap();
    back.sort_by(|a, b| a.name.cmp(&b.name));
    let mut want = values();
    want.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(back, want);
}

fn cmd() -> PathBuf {
    PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\cmd.exe")
}

#[test]
fn waits_for_processes_the_uninstaller_starts() {
    let system = WindowsSystem::new();
    // The first cmd starts a second one that lives about two seconds, then
    // exits at once, as a self-copying uninstaller does.
    let launch = Launch {
        program: cmd(),
        args: r#"/d /c start "" /b cmd.exe /d /c ping -n 3 127.0.0.1 >nul"#.into(),
    };
    let started = Instant::now();
    let mut running = system.launch(&launch).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !running.finished().unwrap(),
        "the started copy is still running"
    );
    while !running.finished().unwrap() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "never finished"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        started.elapsed() >= Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_missing_uninstaller_is_an_error() {
    let system = WindowsSystem::new();
    let launch = Launch {
        program: PathBuf::from(r"C:\nowhere\uninst.exe"),
        args: String::new(),
    };
    assert!(system.launch(&launch).is_err());
}

#[test]
fn reads_this_machines_folders() {
    let system = WindowsSystem::new();
    let folders = wu_core::folders::Folders::load(&system).unwrap();
    assert!(folders.windows.join("System32").is_dir(), "{folders:?}");
    assert!(folders.program_files.is_dir());
    assert!(folders.app_data.is_dir());
}

/// A hive file of our own stands in for another account's NTUSER.DAT:
/// opening a file that does not exist creates an empty hive, which needs no
/// administrator rights.
#[test]
fn another_accounts_hive_is_opened_read_changed_and_released() {
    use wu_core::system::Account;

    let dir = tempfile::tempdir().unwrap();
    let file = super::accounts::hive_file(dir.path(), false);
    {
        let hive = RegKey::load_app_key(&file, false).unwrap();
        hive.create_subkey(r"Software\Acme\Widget")
            .unwrap()
            .0
            .set_value("LastUser", &"other")
            .unwrap();
        hive.create_subkey(r"Software\OtherUserTool").unwrap();
    }
    let system = WindowsSystem::with_profiles(vec![super::accounts::Profile {
        sid: "S-1-5-21-0-0-0-9999".into(),
        account: Account {
            name: "hivetest".into(),
            profile: dir.path().to_path_buf(),
            signed_in: false,
        },
    }]);
    let root = RegPath::account_root("hivetest");
    let key = root.child(r"Software\Acme\Widget");
    assert_eq!(
        system.values(&key).unwrap()[0].as_text().as_deref(),
        Some("other")
    );
    let mut names = system.subkeys(&root.child("Software")).unwrap();
    names.sort();
    assert_eq!(names, ["Acme", "OtherUserTool"]);
    assert!(
        system.delete_key_tree(&root).is_err(),
        "the hive root is never deleted"
    );
    system.delete_key_tree(&key).unwrap();
    system.release_account_hives().unwrap();
    // Closed and reopened on demand: the deletion reached the file.
    assert!(system.subkeys(&key).is_err());
    assert!(
        system
            .subkeys(&root.child(r"Software\OtherUserTool"))
            .is_ok()
    );
    system.release_account_hives().unwrap();
    assert!(
        system
            .values(&RegPath::account_classes_root("hivetest"))
            .is_err(),
        "no classes hive"
    );
}
#[test]
fn hive_keys_outside_known_accounts_are_refused() {
    let system = WindowsSystem::with_profiles(Vec::new());
    let key = RegPath::account_root("nobody").child("Software");
    let error = system.subkeys(&key).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}
