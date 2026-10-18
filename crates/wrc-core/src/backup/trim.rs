//! Revo's "when all backups are larger than": deleting the oldest backups
//! that were made automatically until all of them fit a size.

use std::fs;
use std::path::Path;

use super::{Summary, delete, folder, list};
use crate::error::{Error, Result};

/// Delete the oldest cleaner, search and full backups while all backups
/// together take more than `megabytes`; 0 deletes nothing. The newest
/// backup is always kept, so a clean never loses the backup it just made.
/// Hand backups are never deleted, though they count towards the total.
/// Returns how many were deleted.
pub fn trim(dir: &Path, megabytes: u32) -> Result<usize> {
    if megabytes == 0 {
        return Ok(0);
    }
    let limit = u64::from(megabytes) * 1024 * 1024;
    let summaries = list(dir)?;
    let sizes = summaries
        .iter()
        .map(|summary| folder_size(&folder(dir, &summary.id)?))
        .collect::<Result<Vec<u64>>>()?;
    let mut total: u64 = sizes.iter().sum();
    let mut deleted = 0;
    // `list` is newest first: walk from the oldest, never reaching the newest.
    for (summary, size) in summaries.iter().zip(&sizes).skip(1).rev() {
        if total <= limit {
            break;
        }
        if automatic(summary) {
            delete(dir, &summary.id)?;
            total -= size;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Whether keeping rules may delete this backup: one made by a clean, a
/// search or a full backup, and readable.
pub(super) fn automatic(summary: &Summary) -> bool {
    use super::BackupKind::{Clean, Full, Search};
    matches!(summary.kind, Clean | Search | Full) && summary.damaged.is_none()
}

fn folder_size(path: &Path) -> Result<u64> {
    let read = |source| Error::file("read", path, source);
    let mut total = 0;
    for entry in fs::read_dir(path).map_err(read)? {
        let entry = entry.map_err(read)?;
        let meta = entry.metadata().map_err(read)?;
        total += if meta.is_dir() {
            folder_size(&entry.path())?
        } else {
            meta.len()
        };
    }
    Ok(total)
}
