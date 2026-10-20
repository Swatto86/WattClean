//! The drives a free-space wipe may fill: writable, real file systems
//! (never the system's own read-only volumes, memory-backed folders or
//! snap images), each listed once.

use std::path::{Path, PathBuf};

use wcc_core::system::Drive;

use crate::sys;

/// File systems that hold a person's files on Linux. Not ZFS: a dataset
/// is usually compressed, and stores the zeros a wipe writes as nothing, so
/// the wipe would never fill it (nor a Btrfs or bcachefs mount that
/// compresses, see [`compresses`]).
#[cfg(any(test, not(target_os = "macos")))]
const REAL: &[&str] = &[
    "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "jfs", "reiserfs", "bcachefs", "vfat", "exfat",
    "ntfs", "ntfs3", "fuseblk", "hfsplus", "apfs",
];

/// One line of `/proc/self/mounts`: the device, where it is mounted, the
/// file system, with `\040`-style escapes undone, and whether it is
/// mounted read-only or compresses what is written.
#[cfg(any(test, not(target_os = "macos")))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub device: String,
    pub point: String,
    pub file_system: String,
    pub read_only: bool,
    pub compressed: bool,
}

#[cfg(any(test, not(target_os = "macos")))]
pub fn parse_mounts(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let device = unescape(fields.next()?);
            let point = unescape(fields.next()?);
            let file_system = fields.next()?.to_string();
            let options = fields.next().unwrap_or_default();
            Some(Mount {
                device,
                point,
                file_system,
                read_only: options.split(',').any(|option| option == "ro"),
                compressed: compresses(options),
            })
        })
        .collect()
}

/// Whether mount options turn compression on: Btrfs's `compress` and
/// `compress-force` (as Fedora and Arch installers mount it), bcachefs's
/// `compression`. A wipe's zeros would take a sliver of their size there,
/// so the disk would fill only after many times its free space.
#[cfg(any(test, not(target_os = "macos")))]
fn compresses(options: &str) -> bool {
    options.split(',').any(|option| {
        let (name, value) = option.split_once('=').unwrap_or((option, ""));
        matches!(name, "compress" | "compress-force" | "compression")
            && !matches!(value, "no" | "none")
    })
}

#[cfg(any(test, not(target_os = "macos")))]
/// `/proc/self/mounts` writes a space as `\040`, a tab `\011`, a newline
/// `\012` and a backslash `\134`.
fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut rest = field;
    while let Some(at) = rest.find('\\') {
        out.push_str(&rest[..at]);
        let code = rest.get(at + 1..at + 4);
        match code.and_then(|digits| u8::from_str_radix(digits, 8).ok()) {
            Some(byte) => {
                out.push(char::from(byte));
                rest = &rest[at + 4..];
            }
            None => {
                out.push('\\');
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(any(test, not(target_os = "macos")))]
/// The mounts worth wiping, the first writable mount of each device only
/// (Btrfs mounts one device at several places, and ostree systems mount
/// theirs read-only first).
pub fn wipeable(mounts: &[Mount]) -> Vec<&Mount> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for mount in mounts {
        let real = REAL.contains(&mount.file_system.as_str()) && mount.device.starts_with('/');
        let system = ["/boot", "/efi", "/snap", "/var/lib/snapd"]
            .iter()
            .any(|top| mount.point == *top || mount.point.starts_with(&format!("{top}/")));
        if !real || system || mount.read_only || seen.contains(&mount.device.as_str()) {
            continue;
        }
        // Compression is the file system's, whichever mount shows it.
        seen.push(&mount.device);
        if !mount.compressed {
            out.push(mount);
        }
    }
    out
}

fn drive(root: &str, label: String, file_system: String, removable: bool) -> Option<Drive> {
    let space = sys::space(Path::new(root)).ok()?;
    (!space.read_only && space.total > 0).then(|| Drive {
        root: root.to_string(),
        label,
        file_system,
        free: space.free,
        total: space.total,
        removable,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn drives() -> Vec<Drive> {
    let text = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    let mounts = parse_mounts(&text);
    wipeable(&mounts)
        .into_iter()
        .filter_map(|mount| {
            let removable = ["/media/", "/run/media/", "/mnt/"]
                .iter()
                .any(|top| mount.point.starts_with(top));
            let label = Path::new(&mount.point).file_name().map_or_else(
                || "System".to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            drive(&mount.point, label, mount.file_system.clone(), removable)
        })
        .collect()
}

/// The start-up disk's data volume, then every other volume in `/Volumes`.
/// The system volume itself is read-only, and so is a mounted disk image.
#[cfg(target_os = "macos")]
pub fn drives() -> Vec<Drive> {
    let mut boot_name = "Macintosh HD".to_string();
    let mut others = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/Volumes") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            match std::fs::read_link(&path) {
                Ok(target) if target == Path::new("/") => boot_name = name,
                Ok(_) => {}
                Err(_) if path.is_dir() => others.push((path, name)),
                Err(_) => {}
            }
        }
    }
    let data = if Path::new("/System/Volumes/Data").is_dir() {
        "/System/Volumes/Data"
    } else {
        "/"
    };
    let mut out: Vec<Drive> = drive(data, boot_name, "APFS".to_string(), false)
        .into_iter()
        .collect();
    others.sort();
    for (path, name) in others {
        // Recovery, Preboot and the like are hidden; a network share is not
        // this Mac's disk; a wipe writes at an external volume's root.
        let Ok(mount) = sys::mount_info(&path) else {
            continue;
        };
        if mount.hidden || !mount.local || !sys::can_write(&path) {
            continue;
        }
        out.extend(drive(
            &path.to_string_lossy(),
            name,
            mount.file_system.to_uppercase(),
            true,
        ));
    }
    out
}

/// Where a wipe writes on a drive: a folder this account may write to on
/// that very file system (a memory-backed `/tmp` is a different one), else
/// the drive's root. The device is what counts: on macOS the data volume
/// is mounted at `/System/Volumes/Data` while `/Users`, `/var` and `/tmp`
/// reach it through firmlinks, so no path holds it.
#[cfg(target_os = "macos")]
pub fn wipe_folder(drive: &Drive, candidates: &[PathBuf]) -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let root = Path::new(&drive.root);
    let device = |path: &Path| std::fs::metadata(path).ok().map(|meta| meta.dev());
    let wanted = device(root);
    candidates
        .iter()
        .find(|folder| wanted.is_some() && device(folder) == wanted && sys::can_write(folder))
        .cloned()
        .unwrap_or_else(|| root.to_path_buf())
}

/// Linux: as on macOS, but the device is the one `/proc/self/mounts` names
/// for the mount holding the folder, since Btrfs gives each subvolume
/// (`/home`, `/var` on openSUSE) a number of its own while they all fill
/// the same disk.
#[cfg(not(target_os = "macos"))]
pub fn wipe_folder(drive: &Drive, candidates: &[PathBuf]) -> PathBuf {
    let text = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    let mounts = parse_mounts(&text);
    let root = Path::new(&drive.root);
    candidates
        .iter()
        .find(|folder| {
            std::fs::canonicalize(folder).is_ok_and(|real| same_device(&mounts, root, &real))
                && sys::can_write(folder)
        })
        .cloned()
        .unwrap_or_else(|| root.to_path_buf())
}

/// Whether a folder (its real path, links resolved) is on the device
/// mounted at `root`.
#[cfg(any(test, not(target_os = "macos")))]
fn same_device(mounts: &[Mount], root: &Path, folder: &Path) -> bool {
    let device = |path: &Path| mount_of(mounts, path).map(|mount| mount.device.as_str());
    device(root).is_some_and(|wanted| device(folder) == Some(wanted))
}

/// The mount a path lies on: the one with the longest mount point holding
/// it, the last of those when several share a mount point (each mount
/// hides the one before).
#[cfg(any(test, not(target_os = "macos")))]
fn mount_of<'a>(mounts: &'a [Mount], path: &Path) -> Option<&'a Mount> {
    mounts
        .iter()
        .filter(|mount| path.starts_with(&mount.point))
        .max_by_key(|mount| Path::new(&mount.point).components().count())
}

#[cfg(test)]
#[path = "drives_tests.rs"]
mod tests;
