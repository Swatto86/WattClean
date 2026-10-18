//! What kind of drive a path is on.

use std::ffi::OsStr;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
use wrc_core::system::DriveKind;

use crate::privilege::wide;

// GetDriveTypeW's answers.
const NO_ROOT_DIR: u32 = 1;
const REMOVABLE: u32 = 2;
const FIXED: u32 = 3;
const CDROM: u32 = 5;
const RAMDISK: u32 = 6;

/// The kind of drive `path` (`X:\…`) is on. Anything else, a network
/// drive, or a kind Windows cannot tell counts as a network drive: never
/// checked.
pub fn drive_kind(path: &Path) -> DriveKind {
    let text = path.to_string_lossy();
    let Some(root) = text.get(..3).filter(|root| {
        let bytes = root.as_bytes();
        bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
    }) else {
        return DriveKind::Network;
    };
    let root = wide(OsStr::new(root));
    // SAFETY: `root` is a NUL-terminated wide string that outlives the call.
    match unsafe { GetDriveTypeW(root.as_ptr()) } {
        FIXED | RAMDISK => DriveKind::Fixed,
        REMOVABLE | CDROM => DriveKind::Removable,
        NO_ROOT_DIR => DriveKind::Missing,
        _ => DriveKind::Network,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_drive_is_fixed() {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let windows = format!(r"{system}\Windows");
        assert_eq!(drive_kind(Path::new(&windows)), DriveKind::Fixed);
        assert_eq!(
            drive_kind(Path::new(r"\\server\share\x")),
            DriveKind::Network
        );
        assert_eq!(drive_kind(Path::new("relative")), DriveKind::Network);
    }
}
