//! Removal always backs up first, and restoring puts back exactly what was
//! removed without overwriting anything that came back since.

use super::*;
use crate::fake::FakeSystem;
use crate::model::{Hive, REG_BINARY};
use crate::scan::Category;
use crate::system::System;

pub(super) fn machine() -> (tempfile::TempDir, FakeSystem, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::empty(&dir.path().join("machine"));
    let backups = dir.path().join("backups");
    (dir, system, backups)
}

pub(super) fn target(key: &RegPath, value: Option<&str>) -> Target {
    Target {
        key: key.clone(),
        value: value.map(str::to_owned),
        category: Some(Category::Startup),
        detail: "test".into(),
    }
}

#[test]
fn removes_after_backing_up_and_restores_byte_for_byte() {
    let (_dir, system, backups) = machine();
    let app = RegPath::new(Hive::CurrentUser, r"Software\Fabrikam");
    let blob = RawValue {
        name: "Blob".into(),
        kind: REG_BINARY,
        bytes: vec![0, 1, 254, 255],
    };
    system.put(&app, vec![RawValue::string("", "default"), blob.clone()]);
    system.put(&app.child(r"Deep\Deeper"), vec![RawValue::dword("n", 7)]);
    let run = RegPath::new(Hive::CurrentUser, r"Software\Run");
    system.put(
        &run,
        vec![RawValue::string("Gone", "x"), RawValue::string("Keep", "y")],
    );

    let removal = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "Safe scan",
        vec![target(&app, None), target(&run, Some("Gone"))],
        false,
    )
    .unwrap();
    assert_eq!(removal.removed, 2);
    assert!(removal.failed.is_empty());
    assert!(!system.has_key(&app));
    assert_eq!(system.values_of(&run), vec![RawValue::string("Keep", "y")]);

    let id = removal.backup.unwrap();
    let listed = list(&backups).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].items, listed[0].removed), (2, 2));

    let outcome = restore(&system, &backups, &id, None).unwrap();
    assert_eq!(outcome.restored, 2);
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    assert!(system.values_of(&app).contains(&blob));
    assert_eq!(
        system.values_of(&app.child(r"Deep\Deeper")),
        vec![RawValue::dword("n", 7)]
    );
    assert!(
        system
            .values_of(&run)
            .contains(&RawValue::string("Gone", "x"))
    );

    let again = restore(&system, &backups, &id, None).unwrap();
    assert_eq!(again.restored, 0);
    assert_eq!(again.skipped.len(), 2, "a backup restores once");
}

#[test]
fn restore_never_overwrites_what_came_back() {
    let (_dir, system, backups) = machine();
    let key = RegPath::new(Hive::CurrentUser, r"Software\Run");
    system.put(&key, vec![RawValue::string("App", "old")]);
    let removal = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "x",
        vec![target(&key, Some("App"))],
        false,
    )
    .unwrap();
    system.put(&key, vec![RawValue::string("App", "new")]);
    let outcome = restore(&system, &backups, &removal.backup.unwrap(), None).unwrap();
    assert_eq!(outcome.restored, 0);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(system.values_of(&key), vec![RawValue::string("App", "new")]);
}

/// A protected subkey stops a key's delete part-way, after Windows has
/// already deleted the subkeys before it: the backup is kept and puts back
/// what went, leaving what stayed.
#[test]
fn a_key_delete_stopped_part_way_can_be_put_back() {
    let (_dir, system, backups) = machine();
    let app = RegPath::new(Hive::CurrentUser, r"Software\Fabrikam");
    system.put(&app, vec![RawValue::string("", "app")]);
    system.put(&app.child("Locked"), vec![RawValue::string("a", "1")]);
    system.put(&app.child("Plugins"), vec![RawValue::string("b", "2")]);
    system.protect(&app.child("Locked"));

    let removal = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "Safe scan",
        vec![target(&app, None)],
        false,
    )
    .unwrap();
    assert_eq!(removal.failed.len(), 1);
    assert!(!system.has_key(&app.child("Plugins")), "Windows deleted it");
    let id = removal.backup.expect("the backup holds what was deleted");

    let outcome = restore(&system, &backups, &id, None).unwrap();
    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert_eq!(
        system.values_of(&app.child("Plugins")),
        vec![RawValue::string("b", "2")]
    );
    assert!(system.has_key(&app.child("Locked")));
}

#[test]
fn something_that_cannot_be_backed_up_is_not_removed() {
    let (_dir, system, backups) = machine();
    let missing = RegPath::new(Hive::CurrentUser, r"Software\Nothing");
    let removal = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "x",
        vec![target(&missing, None)],
        false,
    )
    .unwrap();
    assert_eq!(removal.removed, 0);
    assert_eq!(removal.failed.len(), 1);
    assert_eq!(removal.backup, None, "an empty backup is not kept");
    assert!(list(&backups).unwrap().is_empty());
}

#[test]
fn a_value_inside_a_removed_key_goes_with_it() {
    let (_dir, system, backups) = machine();
    let key = RegPath::new(Hive::CurrentUser, r"Software\Fabrikam");
    system.put(&key, vec![RawValue::string("a", "1")]);
    let removal = remove(
        &system,
        &backups,
        BackupKind::Search,
        "x",
        vec![target(&key, Some("a")), target(&key, None)],
        false,
    )
    .unwrap();
    assert_eq!(removal.removed, 1);
    assert!(removal.failed.is_empty());
    let backup = load(&backups, &removal.backup.unwrap()).unwrap();
    assert_eq!(backup.items.len(), 1);
}

#[test]
fn chosen_items_restore_alone() {
    let (_dir, system, backups) = machine();
    let key = RegPath::new(Hive::CurrentUser, r"Software\Run");
    system.put(
        &key,
        vec![RawValue::string("A", "1"), RawValue::string("B", "2")],
    );
    let removal = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "x",
        vec![target(&key, Some("A")), target(&key, Some("B"))],
        false,
    )
    .unwrap();
    let id = removal.backup.unwrap();
    let outcome = restore(&system, &backups, &id, Some(&[1])).unwrap();
    assert_eq!(outcome.restored, 1);
    assert_eq!(system.values_of(&key), vec![RawValue::string("B", "2")]);
    assert!(restore(&system, &backups, &id, Some(&[5])).is_err());
}

#[test]
fn a_hand_backup_puts_back_only_what_is_missing() {
    let (_dir, system, backups) = machine();
    let key = RegPath::new(Hive::CurrentUser, r"Software\Contoso");
    system.put(
        &key,
        vec![RawValue::string("Kept", "1"), RawValue::string("Lost", "2")],
    );
    let summary = back_up_key(&system, &backups, &key, "").unwrap();
    assert_eq!(summary.label, r"HKCU\Software\Contoso");
    system.delete_value(&key, "Lost").unwrap();
    system.put(&key, vec![RawValue::string("Kept", "changed")]);
    let outcome = restore(&system, &backups, &summary.id, None).unwrap();
    assert_eq!(outcome.restored, 1);
    let values = system.values_of(&key);
    assert!(values.contains(&RawValue::string("Kept", "changed")));
    assert!(values.contains(&RawValue::string("Lost", "2")));
}

#[test]
fn ids_from_the_page_cannot_leave_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    for bad in ["..", r"..\x", "a/b", "", "1;2", "C:"] {
        assert!(folder(dir.path(), bad).is_err(), "{bad}");
        assert!(delete(dir.path(), bad).is_err(), "{bad}");
    }
    assert!(folder(dir.path(), "1792110609-2").is_ok());
}

#[test]
fn a_damaged_backup_is_listed_not_hidden() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("1700000000")).unwrap();
    std::fs::write(dir.path().join("1700000000").join(MANIFEST), "{ broken").unwrap();
    let listed = list(dir.path()).unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].damaged.is_some());
    assert_eq!(listed[0].created, 1_700_000_000);
}

#[test]
fn old_cleaner_backups_are_pruned_and_hand_backups_kept() {
    let (_dir, system, backups) = machine();
    let key = RegPath::new(Hive::CurrentUser, r"Software\Old");
    system.put(&key, vec![RawValue::string("a", "1")]);
    let hand = back_up_key(&system, &backups, &key, "hand").unwrap();
    let removal = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "x",
        vec![target(&key, Some("a"))],
        false,
    )
    .unwrap();
    let cleaned = removal.backup.unwrap();
    let created = load(&backups, &cleaned).unwrap().created;
    assert_eq!(
        prune(&backups, 0, created + 100 * 86_400).unwrap(),
        0,
        "0 keeps all"
    );
    assert_eq!(
        prune(&backups, 30, created + 29 * 86_400).unwrap(),
        0,
        "not old enough"
    );
    assert_eq!(prune(&backups, 30, created + 31 * 86_400).unwrap(), 1);
    let left: Vec<String> = list(&backups).unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(left, vec![hand.id]);
}

#[test]
fn trimming_deletes_the_oldest_automatic_backups_until_all_fit() {
    let (_dir, system, backups) = machine();
    let key = RegPath::new(Hive::CurrentUser, r"Software\Big");
    let values = ["a", "b", "c"].map(|name| RawValue::string(name, "1"));
    system.put(&key, values.to_vec());
    let hand = back_up_key(&system, &backups, &key, "hand").unwrap().id;
    let cleaned: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|name| {
            let removal = remove(
                &system,
                &backups,
                BackupKind::Clean,
                "x",
                vec![target(&key, Some(name))],
                false,
            );
            removal.unwrap().backup.unwrap()
        })
        .collect();
    let pad = |id: &str, kb: usize| fs::write(backups.join(id).join("pad"), vec![0; kb << 10]);
    for id in cleaned.iter().chain([&hand]) {
        pad(id, 400).unwrap();
    }
    assert_eq!(trim(&backups, 0).unwrap(), 0, "0 sets no limit");
    assert_eq!(trim(&backups, 1).unwrap(), 2, "1.6 MB down to 1 MB");
    let left: Vec<String> = list(&backups).unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(left, vec![cleaned[2].clone(), hand.clone()]);
    pad(&cleaned[2], 2048).unwrap();
    assert_eq!(
        trim(&backups, 1).unwrap(),
        0,
        "the newest and hand backups stay, even over the limit"
    );
}
