//! Moving every backup to another folder, as Revo's custom data storage
//! folder allows (a full backup is a few hundred megabytes). Each backup is
//! copied first; only when all copied are the originals deleted, so a
//! failure part-way leaves everything where it was.

use std::fs;
use std::path::Path;

use super::folder;
use crate::{Error, Result};

/// Move the backups in `from` into `to`; returns how many moved.
pub fn move_backups(from: &Path, to: &Path) -> Result<usize> {
    if !to.is_absolute() {
        return Err(Error::Invalid("Choose a whole folder path.".into()));
    }
    if to == from {
        return Ok(0);
    }
    if to.starts_with(from) {
        return Err(Error::Invalid(
            "The new folder cannot be inside the current backups folder.".into(),
        ));
    }
    fs::create_dir_all(to).map_err(|source| Error::file("create", to, source))?;
    let ids: Vec<String> = match fs::read_dir(from) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|id| folder(from, id).is_ok())
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(source) => return Err(Error::file("list", from, source)),
    };
    let mut copied = Vec::new();
    for id in &ids {
        let target = to.join(id);
        if target.exists() {
            undo(&copied, to);
            return Err(Error::Invalid(format!(
                "{} already holds a backup {id}.",
                to.display()
            )));
        }
        if let Err(source) = copy_tree(&from.join(id), &target) {
            let _ = fs::remove_dir_all(&target);
            undo(&copied, to);
            return Err(Error::file("copy", from.join(id), source));
        }
        copied.push(id.clone());
    }
    for id in &copied {
        let _ = fs::remove_dir_all(from.join(id));
    }
    Ok(copied.len())
}

fn undo(copied: &[String], to: &Path) {
    for id in copied {
        let _ = fs::remove_dir_all(to.join(id));
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_move_whole_and_a_clash_moves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("backups");
        for id in ["100", "200"] {
            fs::create_dir_all(from.join(id)).unwrap();
            fs::write(from.join(id).join("backup.json"), id).unwrap();
        }
        fs::create_dir_all(from.join("not-a-backup")).unwrap();
        let to = dir.path().join("elsewhere");
        assert_eq!(move_backups(&from, &to).unwrap(), 2);
        assert_eq!(
            fs::read_to_string(to.join("200").join("backup.json")).unwrap(),
            "200"
        );
        assert!(!from.join("100").exists(), "moved, not copied");

        // Moving back onto a folder that already holds one of them.
        fs::create_dir_all(from.join("200")).unwrap();
        let error = move_backups(&to, &from).unwrap_err().to_string();
        assert!(error.contains("already holds"), "{error}");
        assert!(
            to.join("100").exists() && to.join("200").exists(),
            "nothing moved"
        );
        assert!(move_backups(&from, Path::new("relative")).is_err());
        assert!(move_backups(&to, &to.join("inner")).is_err());
    }
}
