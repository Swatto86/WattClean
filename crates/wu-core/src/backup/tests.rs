use super::*;
use crate::model::{Hive, RegPath};
use crate::scan::tests::{machine, uninstall_and_scan};
use crate::scan::{Leftover, Mode, Target};

fn hklm(path: &str) -> RegPath {
    RegPath::new(Hive::LocalMachine, path)
}

#[test]
fn removes_with_backup_then_restores_everything() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Moderate);
    let backups = machine.dir.path().join("backups");
    let key = hklm(r"SOFTWARE\Acme\Widget");
    let before = machine.system.values_of(&key);
    let folder = machine.folders.program_files.join(r"Acme\Widget");
    assert!(folder.is_dir());

    let removal = remove(
        &machine.system,
        &backups,
        "Acme Widget",
        &report.items,
        1_800_000_000,
    )
    .unwrap();
    assert!(removal.failed.is_empty(), "{:?}", removal.failed);
    assert_eq!(removal.removed.total() as usize, report.items.len());
    assert!(
        removal.removed.keys >= 5 && removal.removed.values >= 3 && removal.removed.folders >= 4
    );
    assert!(!machine.system.has_key(&key));
    assert!(!folder.exists());
    assert!(removal.bytes > 0);

    let listed = list(&backups);
    assert_eq!(listed.len(), 1);
    assert!(listed[0].restorable);
    assert_eq!(listed[0].items, report.items.len());

    let restored = restore(&machine.system, &backups, &removal.backup.id, 1_800_000_100).unwrap();
    assert!(restored.failed.is_empty(), "{:?}", restored.failed);
    assert_eq!(restored.restored as usize, report.items.len());
    assert_eq!(machine.system.values_of(&key), before);
    assert!(folder.join("settings.ini").is_file());
    assert!(machine.system.has_key(&hklm(
        r"SOFTWARE\Classes\AcmeWidget.Document\shell\open\command"
    )));

    let again = restore(&machine.system, &backups, &removal.backup.id, 1_800_000_200);
    assert!(again.is_err(), "a backup is restored once");
    let after = &list(&backups)[0];
    assert!(!after.restorable);
    assert_eq!(
        after.items,
        report.items.len(),
        "a restored backup still says what it held"
    );
}

#[test]
fn the_manifest_is_written_before_anything_is_deleted() {
    let machine = machine();
    let key = hklm(r"SOFTWARE\Acme\Widget");
    let gone = hklm(r"SOFTWARE\Nobody\Home");
    let items = vec![
        Leftover {
            id: 1,
            target: Target::Key { key: gone },
            reason: String::new(),
            confidence: crate::scan::Confidence::High,
            selected: true,
            bytes: 0,
        },
        Leftover {
            id: 2,
            target: Target::Key { key: key.clone() },
            reason: String::new(),
            confidence: crate::scan::Confidence::High,
            selected: true,
            bytes: 0,
        },
    ];
    let backups = machine.dir.path().join("backups");
    let removal = remove(&machine.system, &backups, "Test", &items, 5).unwrap();
    assert_eq!(removal.removed.keys, 1);
    assert_eq!(
        removal.failed.len(),
        1,
        "a key that cannot be backed up is not deleted"
    );
    let saved = load(&backups, &removal.backup.id).unwrap();
    assert!(matches!(saved.items[0].state, ItemState::Failed { .. }));
    assert!(saved.items[1].saved.is_some());
}

#[test]
fn restore_never_overwrites_something_new() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Contoso Notes", Mode::Safe);
    let backups = machine.dir.path().join("backups");
    let removal = remove(&machine.system, &backups, "Contoso Notes", &report.items, 7).unwrap();
    let data = machine.folders.app_data.join("Contoso Notes");
    assert!(!data.exists());
    std::fs::create_dir_all(&data).unwrap();
    let restored = restore(&machine.system, &backups, &removal.backup.id, 8).unwrap();
    assert_eq!(restored.failed.len(), 1, "{:?}", restored.failed);
    assert!(restored.failed[0].1.contains("already exists"));
}

#[cfg(windows)]
#[test]
fn a_file_in_use_is_copied_and_left_for_the_next_restart() {
    use std::os::windows::fs::OpenOptionsExt;
    let machine = machine();
    let file = machine.folders.desktop.join("Busy Widget.lnk");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"busy").unwrap();
    // Share read only: Windows refuses to rename or delete it while open.
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&file)
        .unwrap();
    let items = vec![Leftover {
        id: 1,
        target: Target::File { path: file.clone() },
        reason: String::new(),
        confidence: crate::scan::Confidence::High,
        selected: true,
        bytes: 4,
    }];
    let backups = machine.dir.path().join("backups");
    let removal = remove(&machine.system, &backups, "Busy", &items, 9).unwrap();
    drop(handle);
    assert_eq!(removal.at_restart, 1, "{:?}", removal.failed);
    assert_eq!(machine.system.reboot_deletes(), vec![file]);
    let saved = load(&backups, &removal.backup.id).unwrap();
    assert_eq!(saved.items[0].state, ItemState::AtRestart { copied: true });
}

#[test]
fn backup_ids_cannot_escape_the_backups_folder() {
    let dir = tempfile::tempdir().unwrap();
    for bad in ["..", r"..\x", "a/b", "", "C:x", "a b"] {
        assert!(backup_dir(dir.path(), bad).is_err(), "{bad:?}");
        assert!(delete(dir.path(), bad).is_err());
    }
    assert!(backup_dir(dir.path(), "1800000000-acmewidget-2").is_ok());
}
