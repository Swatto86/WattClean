//! The drives a free-space wipe may fill: writable, real file systems
//! (never the system's own read-only volumes, memory-backed folders or
//! snap images), each listed once.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use wcc_core::system::Drive;

use crate::sys;

/// File systems that hold a person's files on Linux.
#[cfg(not(target_os = "macos"))]
const REAL: &[&str] = &[
    "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "jfs", "reiserfs", "zfs", "bcachefs", "vfat",
    "exfat", "ntfs", "ntfs3", "fuseblk", "hfsplus", "apfs",
];

/// One line of `/proc/self/mounts`: the device, where it is mounted, and
/// the file system, with `\040`-style escapes undone.
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub device: String,
    pub point: String,
    pub file_system: String,
}

#[cfg(not(target_os = "macos"))]
pub fn parse_mounts(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some(Mount {
                device: unescape(fields.next()?),
                point: unescape(fields.next()?),
                file_system: fields.next()?.to_string(),
            })
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
/// The mounts worth wiping, the first mount of each device only (Btrfs
/// mounts one device at several places).
pub fn wipeable(mounts: &[Mount]) -> Vec<&Mount> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for mount in mounts {
        let real = REAL.contains(&mount.file_system.as_str()) && mount.device.starts_with('/');
        let system = ["/boot", "/efi", "/snap", "/var/lib/snapd"]
            .iter()
            .any(|top| mount.point == *top || mount.point.starts_with(&format!("{top}/")));
        if real && !system && !seen.contains(&mount.device.as_str()) {
            seen.push(&mount.device);
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
        out.extend(drive(&path.to_string_lossy(), name, String::new(), true));
    }
    out
}

/// Where a wipe writes on a drive: a folder this account may write to on
/// that very file system (a memory-backed `/tmp` is a different one), else
/// the drive's root.
pub fn wipe_folder(drive: &Drive, candidates: &[PathBuf]) -> PathBuf {
    let root = Path::new(&drive.root);
    let device = |path: &Path| std::fs::metadata(path).ok().map(|meta| meta.dev());
    let wanted = device(root);
    candidates
        .iter()
        .filter(|folder| folder.starts_with(root))
        .find(|folder| wanted.is_some() && device(folder) == wanted && sys::can_write(folder))
        .cloned()
        .unwrap_or_else(|| root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    const MOUNTS: &str = "\
/dev/nvme0n1p2 / btrfs rw,subvol=/@ 0 0
proc /proc proc rw 0 0
tmpfs /tmp tmpfs rw 0 0
/dev/nvme0n1p2 /home btrfs rw,subvol=/@home 0 0
/dev/nvme0n1p1 /boot/efi vfat rw 0 0
/dev/loop3 /snap/core/16928 squashfs ro 0 0
/dev/sdb1 /media/t/USB\\040Stick exfat rw 0 0
";

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn real_drives_are_listed_once_with_their_names_unescaped() {
        let mounts = parse_mounts(MOUNTS);
        let points: Vec<&str> = wipeable(&mounts).iter().map(|m| m.point.as_str()).collect();
        assert_eq!(points, ["/", "/media/t/USB Stick"]);
        assert_eq!(unescape(r"a\134b\x"), r"a\b\x");
    }

    #[test]
    fn a_wipe_writes_on_the_drive_itself() {
        let dir = tempfile::tempdir().unwrap();
        let space = sys::space(dir.path()).unwrap();
        let drive = Drive {
            root: dir.path().to_string_lossy().into_owned(),
            label: String::new(),
            file_system: String::new(),
            free: space.free,
            total: space.total,
            removable: false,
        };
        let inside = dir.path().join("inside");
        std::fs::create_dir(&inside).unwrap();
        let elsewhere = PathBuf::from("/");
        assert_eq!(wipe_folder(&drive, &[elsewhere, inside.clone()]), inside);
        assert_eq!(wipe_folder(&drive, &[]), dir.path());
    }

    #[test]
    fn this_machine_has_a_drive_to_wipe() {
        assert!(!drives().is_empty());
    }
}
