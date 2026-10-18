//! Whole hives saved to a file (`RegSaveKeyEx`), as Revo's full backup
//! does. Saving needs the backup privilege, which administrators hold.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, REG_LATEST_FORMAT, RegCloseKey, RegOpenKeyExW,
    RegSaveKeyExW,
};
use wrc_core::model::{Hive, RegPath};

use crate::privilege::{enable_privilege, wide};

fn not_a_hive(key: &RegPath) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{} is not a whole hive", key.display()),
    )
}

fn check(status: u32) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(-1),
        ))
    }
}

/// The hive a key names: its root handle, its name under that root, and
/// its kernel name. `HKCU` and `HKCU\Software\Classes` are the signed-in
/// account's two hives under `HKEY_USERS`.
fn hive(key: &RegPath, sid: Option<&str>) -> io::Result<(HKEY, String, String)> {
    let single = !key.path.is_empty() && !key.path.contains('\\');
    match key.hive {
        Hive::LocalMachine if single => Ok((
            HKEY_LOCAL_MACHINE,
            key.path.clone(),
            format!(r"\REGISTRY\MACHINE\{}", key.path),
        )),
        Hive::Users if single => Ok((
            HKEY_USERS,
            key.path.clone(),
            format!(r"\REGISTRY\USER\{}", key.path),
        )),
        Hive::CurrentUser => {
            let sid =
                sid.ok_or_else(|| io::Error::other("Windows did not say who this account is"))?;
            let name = if key.path.is_empty() {
                sid.to_owned()
            } else if key.path.eq_ignore_ascii_case(r"Software\Classes") {
                format!("{sid}_Classes")
            } else {
                return Err(not_a_hive(key));
            };
            let kernel = format!(r"\REGISTRY\USER\{name}");
            Ok((HKEY_USERS, name, kernel))
        }
        _ => Err(not_a_hive(key)),
    }
}

/// Save the hive at `key` to `file`, which must not exist yet.
pub fn save(key: &RegPath, file: &Path, sid: Option<&str>) -> io::Result<()> {
    enable_privilege("SeBackupPrivilege")?;
    let (root, name, _) = hive(key, sid)?;
    let name = wide(OsStr::new(&name));
    let mut handle: HKEY = std::ptr::null_mut();
    // Read access only: saving needs no more, and the backup-restore
    // option would also ask for write access once the restore privilege is
    // on (after taking ownership or a restore), which SOFTWARE refuses.
    // SAFETY: a predefined root, a NUL-terminated name and a valid out-pointer.
    check(unsafe { RegOpenKeyExW(root, name.as_ptr(), 0, KEY_READ, &mut handle) })?;
    let path = wide(file.as_os_str());
    // SAFETY: `handle` is open; the path is NUL-terminated.
    let saved =
        unsafe { RegSaveKeyExW(handle, path.as_ptr(), std::ptr::null(), REG_LATEST_FORMAT) };
    // SAFETY: opened above and closed once.
    unsafe { RegCloseKey(handle) };
    check(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whole_hives_are_accepted() {
        let sid = Some("S-1-5-21-1");
        let named = |hive, path: &str| {
            super::hive(&RegPath::new(hive, path), sid).map(|(_, name, kernel)| (name, kernel))
        };
        assert_eq!(
            named(Hive::LocalMachine, "SOFTWARE").unwrap().1,
            r"\REGISTRY\MACHINE\SOFTWARE"
        );
        assert_eq!(named(Hive::CurrentUser, "").unwrap().0, "S-1-5-21-1");
        assert_eq!(
            named(Hive::CurrentUser, r"Software\Classes").unwrap().1,
            r"\REGISTRY\USER\S-1-5-21-1_Classes"
        );
        assert_eq!(named(Hive::Users, ".DEFAULT").unwrap().0, ".DEFAULT");
        assert!(named(Hive::LocalMachine, r"SOFTWARE\Microsoft").is_err());
        assert!(named(Hive::CurrentUser, "Software").is_err());
    }
}
