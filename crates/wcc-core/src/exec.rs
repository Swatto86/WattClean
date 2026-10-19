//! Carrying out a clean's operations one by one: the guard is checked
//! again just before each deletion, files may be shredded, and files in use
//! may be queued for deletion at the next restart.

use std::fs;
use std::io;
use std::path::Path;

use crate::edit;
use crate::ops::Op;
use crate::paths::Guard;
use crate::report::Item;
use crate::search::describe;
use crate::shred;
use crate::sqlite;
use crate::system::System;

/// How a clean carries out file deletions.
pub struct Executor<'a> {
    pub system: &'a dyn System,
    pub guard: &'a Guard,
    /// Overwrite file contents before deleting them.
    pub shred: bool,
    /// Ask Windows to delete files in use at the next restart.
    pub delete_on_reboot: bool,
    /// Whether the person pressed Stop (a free-space wipe checks it).
    pub cancelled: &'a dyn Fn() -> bool,
}

/// The result of carrying out one operation.
pub enum Outcome {
    /// Done; the item to report (its size may differ from the plan).
    Done(Item),
    /// Nothing to do any more (already gone, or a folder still in use).
    Skipped,
    Failed {
        message: String,
        in_use: bool,
    },
}

impl Executor<'_> {
    pub fn execute(&self, op: &Op) -> Outcome {
        let mut item = op.item();
        let result: io::Result<bool> = match op {
            Op::DeleteFile { path, .. } => self.delete_file(path, &mut item),
            Op::DeleteDir { path } => self.delete_dir(path),
            Op::Truncate { path, .. } => self.guarded(path).and_then(|()| {
                fs::OpenOptions::new()
                    .write(true)
                    .open(path)?
                    .set_len(0)
                    .map(|()| true)
            }),
            Op::Sqlite { path, op, .. } => self.guarded(path).and_then(|()| {
                sqlite::run(path, *op).map(|saved| {
                    item.size = saved;
                    true
                })
            }),
            Op::Json { path, address } => self
                .guarded(path)
                .and_then(|()| edit::json_remove(path, address)),
            Op::Ini {
                path,
                section,
                parameter,
            } => self
                .guarded(path)
                .and_then(|()| edit::ini_remove(path, section, parameter.as_deref())),
            Op::RegistryKey { key } => self.system.registry().delete_key(key).map(|()| true),
            Op::RegistryValue { key, name } => self
                .system
                .registry()
                .delete_value(key, name)
                .map(|()| true),
            Op::RecycleBin { .. } => self.system.empty_recycle_bin().map(|()| true),
            Op::Clipboard => self.system.clear_clipboard().map(|()| true),
            Op::FreeSpace { folder, .. } => {
                shred::wipe_free_space(folder, self.system.wipe_limit(), self.cancelled, &|_| {})
                    .map(|report| {
                        item.note = Some(format!("{} MB overwritten", report.written >> 20));
                        !report.cancelled
                    })
            }
        };
        match result {
            Ok(true) => Outcome::Done(item),
            Ok(false) => Outcome::Skipped,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Outcome::Skipped,
            Err(error) => Outcome::Failed {
                message: describe(&error),
                in_use: error.raw_os_error() == Some(32),
            },
        }
    }

    fn guarded(&self, path: &Path) -> io::Result<()> {
        self.guard.check(path).map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "protected or on the keep list",
            )
        })
    }

    fn delete_file(&self, path: &Path, item: &mut Item) -> io::Result<bool> {
        self.guarded(path)?;
        let result = if self.shred {
            shred::shred_file(path, &|path| self.system.link_count(path))
        } else {
            remove_file(path)
        };
        match result {
            Err(error) if error.raw_os_error() == Some(32) && self.delete_on_reboot => {
                self.system.delete_on_reboot(path)?;
                item.note = Some("deleted at the next restart".to_string());
                Ok(true)
            }
            other => other.map(|()| true),
        }
    }

    fn delete_dir(&self, path: &Path) -> io::Result<bool> {
        self.guarded(path)?;
        match remove_dir(path) {
            Ok(()) => Ok(true),
            // Something in it was kept, in use or new: leave the folder.
            Err(error)
                if error.raw_os_error() == Some(145)
                    || error.kind() == io::ErrorKind::DirectoryNotEmpty =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}

/// Removes a file, clearing the read-only attribute if that stops it.
pub fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            clear_read_only(path)?;
            fs::remove_file(path)
        }
        other => other,
    }
}

/// Removes an empty folder (or a link to one), clearing read-only if needed.
pub fn remove_dir(path: &Path) -> io::Result<()> {
    match fs::remove_dir(path) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            clear_read_only(path)?;
            fs::remove_dir(path)
        }
        other => other,
    }
}

fn clear_read_only(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    let mut permissions = meta.permissions();
    if !permissions.readonly() {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions)
}
