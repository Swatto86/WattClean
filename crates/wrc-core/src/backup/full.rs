//! A full registry backup: the registry's hives saved whole as files, as
//! Revo Registry Cleaner's full backup does. WattClean does not put them
//! back: having Windows start from saved hive files could not be shown safe
//! on a real PC, and a hive Windows refused at start-up would stop it
//! starting (`FULL_RESTORE`). The SAM and SECURITY hives, which hold
//! password hashes and secrets, are deliberately left out: the backup sits
//! in the person's own folder.

use std::fs;
use std::path::Path;

use super::{Backup, BackupKind, Item, ItemState, Saved, Summary, folder, new_id};
use crate::model::{Hive, RegPath};
use crate::system::System;
use crate::{Error, Result};

/// Why a full backup is not put back from WattClean.
pub const FULL_RESTORE: &str = "A full backup is kept as hive files; WattClean does not put a whole registry back, as a hive Windows could not load would stop it starting. Restore entries from the other backups, or use a System Restore point.";

/// Each hive: where it is mounted, its file in the backup, and what it holds.
const HIVES: [(Hive, &str, &str, &str); 5] = [
    (
        Hive::LocalMachine,
        "SOFTWARE",
        "SOFTWARE",
        "Programs and Windows settings",
    ),
    (
        Hive::LocalMachine,
        "SYSTEM",
        "SYSTEM",
        "Drivers, services and hardware",
    ),
    (
        Hive::Users,
        ".DEFAULT",
        "DEFAULT",
        "The sign-in screen's settings",
    ),
    (Hive::CurrentUser, "", "NTUSER.DAT", "Your settings"),
    (
        Hive::CurrentUser,
        r"Software\Classes",
        "UsrClass.dat",
        "Your file types and shell settings",
    ),
];

/// Save every hive, with administrator rights. A hive that cannot be saved
/// stops the backup and nothing of it is kept.
pub fn back_up_full(system: &dyn System, dir: &Path, label: &str) -> Result<Summary> {
    if !system.is_elevated() {
        return Err(Error::Invalid(
            "A full registry backup needs administrator rights.".into(),
        ));
    }
    let (id, created) = new_id(dir);
    let place = folder(dir, &id)?;
    fs::create_dir_all(&place).map_err(|source| Error::file("create", &place, source))?;
    let mut items = Vec::new();
    for (hive, path, file, what) in HIVES {
        let key = RegPath::new(hive, path);
        let target = place.join(file);
        if let Err(source) = system.save_hive(&key, &target) {
            let _ = fs::remove_dir_all(&place);
            return Err(Error::registry("save", &key, source));
        }
        let size = fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0);
        items.push(Item {
            key,
            value: None,
            category: None,
            detail: format!("{what} ({:.1} MB)", size as f64 / 1_048_576.0),
            saved: Some(Saved::Hive(file.to_owned())),
            security: None,
            state: ItemState::Kept,
        });
    }
    let backup = Backup {
        version: 1,
        id,
        created,
        kind: BackupKind::Full,
        label: if label.trim().is_empty() {
            "Full registry backup".into()
        } else {
            label.trim().chars().take(200).collect()
        },
        items,
    };
    backup.save(dir)?;
    Ok(backup.summary())
}
