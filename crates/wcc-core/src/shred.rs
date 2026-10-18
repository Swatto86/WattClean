//! Overwriting before deleting, so the data cannot be read back from the
//! disk with recovery tools, and wiping a drive's free space.
//!
//! One pass of zeros, as BleachBit does: on modern drives more passes add
//! nothing. On SSDs wear-levelling may keep old copies whatever any program
//! writes, which the window says beside the tool.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::exec as ops;
use crate::model::{FileSpec, Kind, Search};
use crate::paths::Guard;
use crate::report::{PROBLEM_CAP, ProblemLine};
use crate::search::{Filter, Findings, Searcher, describe};

const CHUNK: usize = 1 << 20;
/// Wipe files stay under FAT32's 4 GiB limit.
const WIPE_FILE_LIMIT: u64 = 1 << 30;

/// A name nobody would guess, for renaming before deletion.
fn random_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let low = u64::try_from(nanos & u128::from(u64::MAX)).unwrap_or(0);
    let mut x = low ^ COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    format!("{x:016x}")
}

/// Overwrites a file with zeros, empties it, renames it and deletes it.
/// A link is only unlinked: its target is not this file to destroy.
pub fn shred_file(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return ops::remove_file(path);
    }
    if meta.permissions().readonly() {
        let mut permissions = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    {
        let mut file = OpenOptions::new().write(true).open(path)?;
        // Round up to whole 4 KiB clusters so the file's last cluster goes too.
        let length = meta.len().div_ceil(4096) * 4096;
        let zeros = vec![0u8; CHUNK];
        let mut left = length;
        while left > 0 {
            let n = usize::try_from(left.min(CHUNK as u64)).unwrap_or(CHUNK);
            file.write_all(&zeros[..n])?;
            left -= n as u64;
        }
        file.sync_all()?;
        file.set_len(0)?;
        file.sync_all()?;
    }
    let renamed = path.with_file_name(random_name());
    match fs::rename(path, &renamed) {
        Ok(()) => fs::remove_file(&renamed),
        Err(_) => fs::remove_file(path),
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShredReport {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub problems: Vec<ProblemLine>,
    pub cancelled: bool,
}

/// Shreds files and folders (with everything in them) chosen by a person.
pub fn shred_paths(
    paths: &[PathBuf],
    guard: &Guard,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(&ShredReport, &Path),
) -> ShredReport {
    let mut report = ShredReport::default();
    let searcher = Searcher { guard, cancelled };
    for path in paths {
        let spec = FileSpec::new(path.to_string_lossy(), Search::WalkTop);
        let Ok(filter) = Filter::new(&spec, &|_: &str| Vec::new(), SystemTime::now()) else {
            continue;
        };
        let mut findings = Findings::default();
        searcher.find(&spec, &filter, &|_: &str| Vec::new(), &mut findings);
        for problem in findings.problems {
            push_problem(&mut report, &problem.path, problem.message);
        }
        if findings.found.is_empty() && guard.check(path).is_err() {
            push_problem(
                &mut report,
                path,
                "protected or on the keep list".to_string(),
            );
        }
        for found in findings.found {
            if cancelled() {
                report.cancelled = true;
                return report;
            }
            let result = match found.kind {
                Kind::File => shred_file(&found.path),
                Kind::Dir => ops::remove_dir(&found.path),
            };
            match result {
                Ok(()) if found.kind == Kind::File => {
                    report.files += 1;
                    report.bytes += found.size;
                }
                Ok(()) => report.folders += 1,
                Err(error) => push_problem(&mut report, &found.path, describe(&error)),
            }
            progress(&report, &found.path);
        }
    }
    report.cancelled = cancelled();
    report
}

fn push_problem(report: &mut ShredReport, path: &Path, message: String) {
    if report.problems.len() < PROBLEM_CAP {
        report.problems.push(ProblemLine {
            path: path.to_string_lossy().to_string(),
            message,
        });
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WipeReport {
    pub written: u64,
    pub cancelled: bool,
    pub elapsed_ms: u64,
}

fn disk_full(error: &io::Error) -> bool {
    // ERROR_HANDLE_DISK_FULL, ERROR_DISK_FULL, and the portable kind.
    matches!(error.raw_os_error(), Some(39 | 112)) || error.kind() == io::ErrorKind::StorageFull
}

/// Fills the free space of the drive holding `folder` with zeros, then
/// deletes what it wrote. Stops at `limit` bytes when one is given.
/// `progress` gets the bytes written so far.
pub fn wipe_free_space(
    folder: &Path,
    limit: Option<u64>,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(u64),
) -> io::Result<WipeReport> {
    let started = std::time::Instant::now();
    let work = folder.join(format!("WattClean-wipe-{}", random_name()));
    fs::create_dir(&work)?;
    let result = fill(&work, limit, cancelled, progress);
    // Whatever happened, give the space back.
    let cleanup = fs::remove_dir_all(&work);
    let (written, stopped) = result?;
    cleanup?;
    Ok(WipeReport {
        written,
        cancelled: stopped,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// Returns the bytes written and whether it was cancelled.
fn fill(
    work: &Path,
    limit: Option<u64>,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(u64),
) -> io::Result<(u64, bool)> {
    let zeros = vec![0u8; CHUNK];
    let mut written = 0u64;
    for index in 0u32.. {
        let mut file = File::create(work.join(format!("fill{index:05}.bin")))?;
        let mut in_file = 0u64;
        loop {
            if cancelled() {
                return Ok((written, true));
            }
            let room = limit.map_or(CHUNK as u64, |l| {
                l.saturating_sub(written).min(CHUNK as u64)
            });
            if room == 0 {
                file.sync_all()?;
                return Ok((written, false));
            }
            let n = usize::try_from(room).unwrap_or(CHUNK);
            match file.write_all(&zeros[..n]) {
                Ok(()) => {
                    written += n as u64;
                    in_file += n as u64;
                    progress(written);
                }
                Err(error) if disk_full(&error) => {
                    // Flushing may fail too on a full disk; the data is out.
                    let _flushed = file.sync_all();
                    return Ok((written, false));
                }
                Err(error) => return Err(error),
            }
            if in_file >= WIPE_FILE_LIMIT {
                file.sync_all()?;
                break;
            }
        }
    }
    Ok((written, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shredding_overwrites_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("secret.txt");
        fs::write(&file, b"top secret").unwrap();
        shred_file(&file).unwrap();
        assert!(!file.exists());
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            0,
            "no renamed copy left"
        );
    }

    #[test]
    fn shredding_folders_takes_their_contents() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Private");
        fs::create_dir_all(folder.join("deeper")).unwrap();
        fs::write(folder.join("a.txt"), b"aaaa").unwrap();
        fs::write(folder.join("deeper").join("b.txt"), b"bb").unwrap();
        let loose = dir.path().join("loose.txt");
        fs::write(&loose, b"c").unwrap();
        let report = shred_paths(
            &[folder.clone(), loose.clone()],
            &Guard::default(),
            &|| false,
            &|_, _| {},
        );
        assert_eq!((report.files, report.folders, report.bytes), (3, 2, 7));
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(!folder.exists() && !loose.exists());
    }

    #[test]
    fn kept_paths_are_not_shredded() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("keep.txt");
        fs::write(&file, b"x").unwrap();
        let guard = Guard::new(&[], std::slice::from_ref(&file));
        let report = shred_paths(std::slice::from_ref(&file), &guard, &|| false, &|_, _| {});
        assert_eq!(report.files, 0);
        assert_eq!(report.problems.len(), 1);
        assert!(file.exists());
    }

    #[test]
    fn wiping_writes_up_to_the_limit_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let seen = AtomicU64::new(0);
        let report = wipe_free_space(dir.path(), Some(3 * CHUNK as u64 + 10), &|| false, &|n| {
            seen.store(n, Ordering::Relaxed);
        })
        .unwrap();
        assert_eq!(report.written, 3 * CHUNK as u64 + 10);
        assert_eq!(seen.load(Ordering::Relaxed), report.written);
        assert!(!report.cancelled);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_cancelled_wipe_still_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let report = wipe_free_space(dir.path(), Some(1 << 30), &|| true, &|_| {}).unwrap();
        assert!(report.cancelled);
        assert_eq!(report.written, 0);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
