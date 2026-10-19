//! What kind of drive a path is on.

use std::ffi::OsStr;
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, GetLastError};
use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, QueryDosDeviceW};
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;
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
    let letter = &text[..2];
    let root = wide(OsStr::new(root));
    // SAFETY: `root` is a NUL-terminated wide string that outlives the call.
    let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
    of_type(kind, || letter_in_use(letter))
}

fn of_type(kind: u32, in_use: impl FnOnce() -> bool) -> DriveKind {
    match kind {
        FIXED | RAMDISK => DriveKind::Fixed,
        REMOVABLE | CDROM => DriveKind::Removable,
        NO_ROOT_DIR if !in_use() => DriveKind::Missing,
        _ => DriveKind::Network,
    }
}

/// Whether a letter with no drive behind it is still taken: a `subst`
/// whose folder is gone, or a network drive the person mapped. An elevated
/// process does not see the mappings of the person's unelevated session,
/// so the ones Windows remembers (`HKCU\Network`) count too. Anything
/// unclear counts as taken.
fn letter_in_use(letter: &str) -> bool {
    let device = wide(OsStr::new(letter));
    let mut target = [0u16; 1024];
    let size = u32::try_from(target.len()).unwrap_or(u32::MAX);
    // SAFETY: `device` is NUL-terminated and `target` holds `size` units.
    let written = unsafe { QueryDosDeviceW(device.as_ptr(), target.as_mut_ptr(), size) };
    // SAFETY: reads this thread's last error, set by the call above.
    if written != 0 || unsafe { GetLastError() } != ERROR_FILE_NOT_FOUND {
        return true;
    }
    match RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!(r"Network\{}", &letter[..1])) {
        Ok(_) => true,
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
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

    #[test]
    fn a_letter_with_no_drive_is_missing_only_when_nothing_holds_it() {
        assert_eq!(of_type(NO_ROOT_DIR, || false), DriveKind::Missing);
        assert_eq!(of_type(NO_ROOT_DIR, || true), DriveKind::Network);
        assert_eq!(of_type(FIXED, || panic!("not asked")), DriveKind::Fixed);
        assert_eq!(of_type(REMOVABLE, || true), DriveKind::Removable);
    }

    #[test]
    #[ignore = "defines a drive letter in this logon session for a moment"]
    fn a_subst_letter_whose_folder_is_gone_is_not_missing() {
        use windows_sys::Win32::Storage::FileSystem::{
            DDD_EXACT_MATCH_ON_REMOVE, DDD_REMOVE_DEFINITION, DefineDosDeviceW,
        };
        let letter = ('M'..='Y')
            .rev()
            .find(|letter| {
                let root = wide(OsStr::new(&format!(r"{letter}:\")));
                // SAFETY: `root` is a NUL-terminated wide string.
                unsafe { GetDriveTypeW(root.as_ptr()) == NO_ROOT_DIR }
            })
            .expect("a free drive letter");
        let path = format!(r"{letter}:\Tools\x.exe");
        assert_eq!(drive_kind(Path::new(&path)), DriveKind::Missing);
        let gone = std::env::temp_dir().join("wattclean-no-such-folder-for-subst");
        let device = wide(OsStr::new(&format!("{letter}:")));
        let target = wide(gone.as_os_str());
        // SAFETY: both are NUL-terminated wide strings that outlive the call.
        let defined = unsafe { DefineDosDeviceW(0, device.as_ptr(), target.as_ptr()) };
        assert_ne!(defined, 0);
        let kind = drive_kind(Path::new(&format!(r"{letter}:\Tools\x.exe")));
        // SAFETY: as above; removes exactly the definition made here.
        unsafe {
            DefineDosDeviceW(
                DDD_REMOVE_DEFINITION | DDD_EXACT_MATCH_ON_REMOVE,
                device.as_ptr(),
                target.as_ptr(),
            )
        };
        assert_eq!(kind, DriveKind::Network);
    }
}
