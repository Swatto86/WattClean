//! Protected keys: removed only with "Take ownership", and restored with
//! the protection they had.

use super::tests::{machine, target};
use super::*;
use crate::model::Hive;

#[test]
fn protected_keys_need_force_and_restore_their_protection() {
    use crate::fake::{OWNED, PROTECTED};
    let (_dir, system, backups) = machine();
    let key = RegPath::new(Hive::LocalMachine, r"SOFTWARE\Classes\CLSID\{1}");
    system.put(
        &key.child("InprocServer32"),
        vec![RawValue::string("", "x.dll")],
    );
    system.protect(&key);
    let plain = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "x",
        vec![target(&key, None)],
        false,
    )
    .unwrap();
    assert_eq!(plain.removed, 0);
    assert!(
        plain.failed[0].1.contains("Take ownership"),
        "{:?}",
        plain.failed
    );
    assert!(system.has_key(&key));

    let forced = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "x",
        vec![target(&key, None)],
        true,
    )
    .unwrap();
    assert_eq!(
        (forced.removed, forced.forced),
        (1, 1),
        "{:?}",
        forced.failed
    );
    assert!(!system.has_key(&key));
    let id = forced.backup.unwrap();
    let outcome = restore(&system, &backups, &id, None).unwrap();
    assert_eq!(outcome.restored, 1);
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    assert_eq!(system.security_of(&key).as_deref(), Some(PROTECTED));
    assert_eq!(
        system.security_of(&key.child("InprocServer32")).as_deref(),
        Some(PROTECTED)
    );
    assert_ne!(PROTECTED, OWNED);
}

#[test]
fn a_protected_keys_value_goes_and_the_key_stays_protected() {
    use crate::fake::PROTECTED;
    let (_dir, system, backups) = machine();
    let key = RegPath::new(Hive::LocalMachine, r"SOFTWARE\Protected");
    system.put(
        &key,
        vec![RawValue::string("Gone", "1"), RawValue::string("Keep", "2")],
    );
    system.protect(&key);
    let removal = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "x",
        vec![target(&key, Some("Gone"))],
        true,
    )
    .unwrap();
    assert_eq!((removal.removed, removal.forced), (1, 1));
    assert!(removal.warnings.is_empty(), "{:?}", removal.warnings);
    assert_eq!(system.values_of(&key), vec![RawValue::string("Keep", "2")]);
    assert_eq!(system.security_of(&key).as_deref(), Some(PROTECTED));
    let outcome = restore(&system, &backups, &removal.backup.unwrap(), None).unwrap();
    assert_eq!(outcome.restored, 1);
    assert!(
        system
            .values_of(&key)
            .contains(&RawValue::string("Gone", "1"))
    );
    assert_eq!(system.security_of(&key).as_deref(), Some(PROTECTED));
}

#[test]
fn without_administrator_rights_force_changes_nothing() {
    let (_dir, system, backups) = machine();
    let key = RegPath::new(Hive::LocalMachine, r"SOFTWARE\Protected");
    system.put(&key, vec![RawValue::string("a", "1")]);
    system.protect(&key);
    system.set_elevated(false);
    let removal = remove(
        &system,
        &backups,
        BackupKind::Clean,
        "x",
        vec![target(&key, None)],
        true,
    )
    .unwrap();
    assert_eq!(removal.removed, 0);
    assert!(system.has_key(&key));
    assert_eq!(
        system.security_of(&key).as_deref(),
        Some(crate::fake::PROTECTED)
    );
}
