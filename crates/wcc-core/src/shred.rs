//! Overwriting before deleting, so the data cannot be read back from the
//! disk with recovery tools, and wiping a drive's free space.
//!
//! One pass of zeros, as BleachBit does: on modern drives more passes add
//! nothing. On SSDs wear-levelling may keep old copies whatever any program
//! writes, which the window says beside the tool.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::exec as ops;
use crate::model::{FileSpec, Kind, Search};
use crate::paths::{self, Guard};
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

/// How many names a file's data has (hard links); `System::link_count`.
pub type LinkCount<'a> = &'a dyn Fn(&Path) -> io::Result<u32>;

/// Renames a file to a random name, so its name does not stay on the disk
/// either, overwrites it with zeros, empties it and deletes it. A link is
/// only unlinked: its target is not this file to destroy. A file whose data
/// has other names (hard links, as Windows' own files have) is refused:
/// overwriting it would destroy the data under every name.
pub fn shred_file(path: &Path, links: LinkCount<'_>) -> io::Result<()> {
    shred_with(path, links, &|path| fs::remove_file(path))
}

/// [`shred_file`], deleting with `remove` (the tests' way of refusing).
fn shred_with(
    path: &Path,
    links: LinkCount<'_>,
    remove: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return ops::remove_file(path);
    }
    if links(path)? > 1 {
        return Err(io::Error::other(
            "not shredded: other hard links share this file's data",
        ));
    }
    // Renamed first: a folder that will not let the file be renamed will
    // not let it be deleted either, and then it is left exactly as it was
    // rather than emptied in place.
    let renamed = path.with_file_name(random_name());
    fs::rename(path, &renamed)?;
    let mut changed = None;
    let result = open_writable(&renamed, &meta.permissions(), &mut changed)
        .and_then(|file| overwrite(file, meta.len()))
        .and_then(|()| remove(&renamed));
    let Err(error) = result else {
        return Ok(());
    };
    // Its own name and permissions back, so the problem names a file the
    // person knows, as it was apart from what was overwritten.
    if let Some(permissions) = changed {
        let _restored = fs::set_permissions(&renamed, permissions);
    }
    match fs::rename(&renamed, path) {
        Ok(()) => Err(error),
        Err(_) => Err(io::Error::new(
            error.kind(),
            format!("{error}; the file is left as {}", renamed.display()),
        )),
    }
}

/// Opens a file to overwrite it. One this account may not write is made
/// writable first, and `changed` keeps the permissions it had.
fn open_writable(
    path: &Path,
    permissions: &fs::Permissions,
    changed: &mut Option<fs::Permissions>,
) -> io::Result<File> {
    match ops::open_to_overwrite(path) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            let Some(writable) = writable(permissions) else {
                return Err(error);
            };
            fs::set_permissions(path, writable)?;
            *changed = Some(permissions.clone());
            ops::open_to_overwrite(path)
        }
        other => other,
    }
}

/// The permissions that let the owner write the file, when it cannot. On
/// Linux and macOS only the owner's write bit is added: clearing
/// "read-only" would let every account write it.
#[cfg(unix)]
fn writable(permissions: &fs::Permissions) -> Option<fs::Permissions> {
    use std::os::unix::fs::PermissionsExt;
    let mode = permissions.mode() & 0o7777;
    (mode & 0o200 == 0).then(|| fs::Permissions::from_mode(mode | 0o200))
}

#[cfg(not(unix))]
fn writable(permissions: &fs::Permissions) -> Option<fs::Permissions> {
    let mut writable = permissions.clone();
    #[allow(clippy::permissions_set_readonly_false)]
    writable.set_readonly(false);
    permissions.readonly().then_some(writable)
}

/// Writes zeros over the file's length, then empties it.
fn overwrite(mut file: File, length: u64) -> io::Result<()> {
    // Round up to whole 4 KiB clusters so the file's last cluster goes too.
    let mut left = length.div_ceil(4096) * 4096;
    let zeros = vec![0u8; CHUNK];
    while left > 0 {
        let n = usize::try_from(left.min(CHUNK as u64)).unwrap_or(CHUNK);
        file.write_all(&zeros[..n])?;
        left -= n as u64;
    }
    file.sync_all()?;
    file.set_len(0)?;
    file.sync_all()
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShredReport {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub problems: Vec<ProblemLine>,
    pub cancelled: bool,
    /// The chosen files and folders still there afterwards (something in
    /// them was kept, refused or not reached), as they were given, so the
    /// window can keep them queued without rebuilding that from problems.
    pub left: Vec<String>,
}

/// Shreds files and folders (with everything in them) chosen by a person,
/// each taken exactly as given (`Searcher::find_literal`).
pub fn shred_paths(
    paths: &[PathBuf],
    guard: &Guard,
    links: LinkCount<'_>,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(&ShredReport, &Path),
) -> ShredReport {
    let mut report = ShredReport::default();
    let searcher = Searcher { guard, cancelled };
    for (index, path) in paths.iter().enumerate() {
        let spec = FileSpec::chosen(path.to_string_lossy(), Search::WalkTop);
        let Ok(filter) = Filter::new(&spec, &|_: &str| Vec::new(), SystemTime::now()) else {
            continue;
        };
        let mut findings = Findings::default();
        searcher.find_literal(path, spec.search, &filter, &mut findings);
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
                report.left.extend(still_there(&paths[index..]));
                return report;
            }
            let result = match found.kind {
                Kind::File => shred_file(&found.path, links),
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
        report.left.extend(still_there(std::slice::from_ref(path)));
    }
    report.cancelled = cancelled();
    report
}

/// The paths that still name something, as they were given.
fn still_there(paths: &[PathBuf]) -> impl Iterator<Item = String> + '_ {
    paths
        .iter()
        .filter(|path| {
            fs::symlink_metadata(paths::chosen(path))
                .map_or_else(|e| e.kind() != io::ErrorKind::NotFound, |_| true)
        })
        .map(|path| path.to_string_lossy().to_string())
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
    let work = folder.join(format!("{FILLER_PREFIX}{}", random_name()));
    fs::create_dir(&work)?;
    let result = fill(&work, limit, cancelled, progress);
    // Whatever happened, give the space back.
    let cleanup = remove_filler(&work);
    let (written, stopped) = result?;
    cleanup?;
    Ok(WipeReport {
        written,
        cancelled: stopped,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// What every wipe names its filler folder.
const FILLER_PREFIX: &str = "WattClean-wipe-";

/// Removes a filler folder. A virus scanner reading the files just written
/// can hold them for a moment, so a refusal is tried again for a few
/// seconds before giving up: until it goes, the drive stays full.
fn remove_filler(work: &Path) -> io::Result<()> {
    let mut wait = Duration::from_millis(100);
    let mut tries = 1;
    loop {
        match fs::remove_dir_all(work) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) if tries < 8 => {
                std::thread::sleep(wait);
                wait = (wait * 2).min(Duration::from_secs(2));
                tries += 1;
            }
            other => return other,
        }
    }
}

/// A wipe cut short (power loss, a killed process) leaves its filler
/// folder in `folder`; removes any found so the space comes back. Returns
/// what could not be removed.
pub fn remove_wipe_leftovers(folder: &Path) -> Vec<(PathBuf, io::Error)> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(FILLER_PREFIX)
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .filter_map(|entry| {
            let path = entry.path();
            remove_filler(&path).err().map(|error| (path, error))
        })
        .collect()
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
#[path = "shred_tests.rs"]
mod tests;
