//! Adapter tests against private registry hives, so the machine's own
//! registry is never touched.

use std::path::Path;
use std::time::Duration;

use winreg::RegKey;
use wrc_core::backup::{BackupKind, Target, remove, restore};
use wrc_core::model::{REG_BINARY, REG_MULTI_SZ, REG_QWORD, RawValue, RegPath};
use wrc_core::scan::{Category, Mode, Options, Progress, Request, scan};
use wrc_core::{Hive, System};

use super::WindowsSystem;

fn hive_system(dir: &Path) -> WindowsSystem {
    let machine = RegKey::load_app_key(dir.join("machine.hive"), false).unwrap();
    let user = RegKey::load_app_key(dir.join("user.hive"), false).unwrap();
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
fn removal_and_restore_round_trip_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let system = hive_system(dir.path());
    let key = RegPath::new(Hive::CurrentUser, r"Software\Fabrikam\Widget");
    for value in values() {
        system.write_value(&key, &value).unwrap();
    }
    system.create_key(&key.child("Child")).unwrap();
    let backups = dir.path().join("backups");
    let target = Target {
        key: key.clone(),
        value: None,
        category: None,
        detail: "test".into(),
    };
    let removal = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "test",
        vec![target],
        false,
    )
    .unwrap();
    assert_eq!(removal.removed, 1, "{:?}", removal.failed);
    assert!(system.subkeys(&key).is_err(), "the key is gone");
    let outcome = restore(&system, &backups, &removal.backup.unwrap(), None).unwrap();
    assert_eq!(outcome.restored, 1);
    let mut read = system.values(&key).unwrap();
    read.sort_by(|a, b| a.name.cmp(&b.name));
    let mut want = values();
    want.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(read, want);
    assert_eq!(system.subkeys(&key).unwrap(), vec!["Child".to_owned()]);
}

#[test]
fn a_watch_is_signalled_by_a_change_beneath_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let system = hive_system(dir.path());
    let key = RegPath::new(Hive::CurrentUser, r"Software\Watched");
    system.create_key(&key.child("Deep")).unwrap();
    let mut watch = system.watch_key(&key).unwrap();
    assert!(
        !watch.wait(Duration::from_millis(100)).unwrap(),
        "nothing changed yet"
    );
    system
        .write_value(&key.child("Deep"), &RawValue::dword("n", 1))
        .unwrap();
    assert!(
        watch.wait(Duration::from_secs(5)).unwrap(),
        "a change below was missed"
    );
    // Re-armed: the next change is signalled too.
    system.delete_value(&key.child("Deep"), "n").unwrap();
    assert!(
        watch.wait(Duration::from_secs(5)).unwrap(),
        "the second change was missed"
    );
    assert!(!watch.wait(Duration::from_millis(100)).unwrap());
}

#[test]
fn a_scan_reads_the_real_registry_api() {
    let dir = tempfile::tempdir().unwrap();
    let system = hive_system(dir.path());
    let present = dir.path().join("present.exe");
    std::fs::write(&present, b"x").unwrap();
    let gone = dir.path().join("gone.exe");
    let run = RegPath::new(
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Run",
    );
    system
        .write_value(
            &run,
            &RawValue::string("Here", &format!("\"{}\" /x", present.display())),
        )
        .unwrap();
    system
        .write_value(
            &run,
            &RawValue::string("Gone", &format!("\"{}\" /x", gone.display())),
        )
        .unwrap();
    let request = Request {
        mode: Mode::Custom,
        categories: vec![Category::Startup],
        all_accounts: false,
    };
    let options = Options {
        exclude_removable: true,
        exclude_windows: true,
        ..Options::default()
    };
    let report = scan(&system, &request, &options, &Progress::default());
    let found: Vec<_> = report.findings.iter().map(|f| f.value.clone()).collect();
    assert_eq!(found, vec![Some("Gone".to_owned())]);
}

/// Application hives refuse permission changes, so taking ownership is
/// checked on the real registry by `examples/force_check.rs` instead; here
/// only reading a key's owner and permissions is.
#[test]
fn a_keys_owner_and_permissions_read_as_sddl() {
    let dir = tempfile::tempdir().unwrap();
    let system = hive_system(dir.path());
    let key = RegPath::new(Hive::CurrentUser, r"Software\Owned");
    system.create_key(&key).unwrap();
    let sddl = system.key_security(&key).unwrap();
    assert!(sddl.starts_with("O:S-1-5-"), "{sddl}");
    assert!(sddl.contains("D:"), "{sddl}");
}
