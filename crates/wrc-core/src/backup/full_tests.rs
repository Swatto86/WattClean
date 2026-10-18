//! A full backup saves every hive; WattClean does not put them back.

use super::*;
use crate::fake::FakeSystem;
use crate::model::Hive;

fn machine() -> (tempfile::TempDir, FakeSystem, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::empty(&dir.path().join("machine"));
    let backups = dir.path().join("backups");
    (dir, system, backups)
}

#[test]
fn a_full_backup_saves_every_hive_and_is_not_put_back_here() {
    let (_dir, system, backups) = machine();
    system.put(
        &RegPath::new(Hive::LocalMachine, r"SOFTWARE\Acme"),
        vec![RawValue::string("a", "1")],
    );
    system.set_elevated(false);
    assert!(
        back_up_full(&system, &backups, "").is_err(),
        "needs administrator rights"
    );
    system.set_elevated(true);
    let summary = back_up_full(&system, &backups, "").unwrap();
    assert_eq!(summary.kind, BackupKind::Full);
    assert_eq!(summary.label, "Full registry backup");
    assert_eq!(summary.items, 5);
    let place = backups.join(&summary.id);
    for file in [
        "SOFTWARE",
        "SYSTEM",
        "DEFAULT",
        "NTUSER.DAT",
        "UsrClass.dat",
    ] {
        assert!(place.join(file).is_file(), "{file} missing");
    }
    let software = std::fs::read_to_string(place.join("SOFTWARE")).unwrap();
    assert!(software.contains("Acme"), "the hive's keys are in its file");

    // Not put back from WattClean: a hive Windows refused would stop it starting.
    let refused = restore(&system, &backups, &summary.id, None).unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("does not put a whole registry back")
    );
    assert!(to_reg_file(&load(&backups, &summary.id).unwrap()).contains("is a whole hive"));
}

#[test]
fn old_full_backups_are_pruned_too() {
    let (_dir, system, backups) = machine();
    let summary = back_up_full(&system, &backups, "").unwrap();
    let later = summary.created + 31 * 86_400;
    assert_eq!(prune(&backups, 30, later).unwrap(), 1);
    assert!(list(&backups).unwrap().is_empty());
}
