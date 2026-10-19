//! The C library calls this crate needs, each behind a safe function.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// `ST_RDONLY`: the same bit on Linux and macOS.
const ST_RDONLY: u64 = 1;

/// Sizes of the file system holding a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    /// Bytes free to an ordinary account.
    pub free: u64,
    pub total: u64,
    pub read_only: bool,
}

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a path holds a NUL byte"))
}

/// Whether this program runs as root.
pub fn is_elevated() -> bool {
    // SAFETY: geteuid takes no arguments and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

/// This account's user id.
#[cfg(not(target_os = "macos"))]
pub fn uid() -> u32 {
    // SAFETY: getuid takes no arguments and cannot fail.
    unsafe { libc::getuid() }
}

#[allow(
    clippy::useless_conversion,
    reason = "statvfs fields are u64 on Linux but u32 on macOS"
)]
pub fn space(path: &Path) -> io::Result<Space> {
    let c = c_path(path)?;
    // SAFETY: statvfs is plain data; all-zero bytes are a valid value.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path and `stat` a writable statvfs
    // that outlives the call.
    if unsafe { libc::statvfs(c.as_ptr(), &raw mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // The field types differ between Linux and macOS (u32 or u64); every
    // one of them widens to u64 without loss.
    let block = u64::from(stat.f_frsize);
    Ok(Space {
        free: u64::from(stat.f_bavail).saturating_mul(block),
        total: u64::from(stat.f_blocks).saturating_mul(block),
        read_only: u64::from(stat.f_flag) & ST_RDONLY != 0,
    })
}

/// How macOS mounted a volume.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// Not shown in Finder (Recovery, Preboot, VM, Update).
    pub hidden: bool,
    /// On this Mac, not a network share.
    pub local: bool,
    /// `apfs`, `hfs`, `exfat`, `msdos`…
    pub file_system: String,
}

#[cfg(target_os = "macos")]
pub fn mount_info(path: &Path) -> io::Result<Mount> {
    const MNT_LOCAL: u32 = 0x0000_1000;
    const MNT_DONTBROWSE: u32 = 0x0010_0000;
    let c = c_path(path)?;
    // SAFETY: statfs is plain data; all-zero bytes are a valid value.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path and `stat` a writable statfs that
    // outlives the call.
    if unsafe { libc::statfs(c.as_ptr(), &raw mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let name: Vec<u8> = stat
        .f_fstypename
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| c.to_ne_bytes()[0])
        .collect();
    Ok(Mount {
        hidden: stat.f_flags & MNT_DONTBROWSE != 0,
        local: stat.f_flags & MNT_LOCAL != 0,
        file_system: String::from_utf8_lossy(&name).into_owned(),
    })
}

/// The account's own temporary folder (`/var/folders/…/T`), which macOS
/// knows even when `TMPDIR` is not set (ssh, cron, launchd).
#[cfg(target_os = "macos")]
pub fn user_temp_dir() -> Option<String> {
    const CS_DARWIN_USER_TEMP_DIR: libc::c_int = 65537;
    let mut buf = vec![0u8; 1024];
    // SAFETY: `buf` is writable for `buf.len()` bytes and confstr writes at
    // most that many, NUL included.
    let n = unsafe { libc::confstr(CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr().cast(), buf.len()) };
    if n == 0 || n > buf.len() {
        return None;
    }
    buf.truncate(n - 1);
    let dir = String::from_utf8(buf).ok()?;
    let dir = dir.trim_end_matches('/').to_string();
    (dir.starts_with('/') && dir != "/").then_some(dir)
}

/// Whether this account may create files in a folder.
pub fn can_write(path: &Path) -> bool {
    let Ok(c) = c_path(path) else {
        return false;
    };
    // SAFETY: `c` is a NUL-terminated path; access only reads it.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// Calls `handler` on SIGINT and SIGTERM instead of ending the program.
pub fn on_interrupt(handler: extern "C" fn(libc::c_int)) -> io::Result<()> {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: `handler` is a plain function that lives as long as the
        // process and only stores to an atomic, which is signal-safe.
        let previous = unsafe { libc::signal(signal, handler as libc::sighandler_t) };
        if previous == libc::SIG_ERR {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_file_system_has_a_size() {
        let root = space(Path::new("/")).unwrap();
        assert!(root.total > 0 && root.free <= root.total);
        assert!(space(Path::new("/no/such/folder")).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_knows_the_accounts_temporary_folder_and_its_volumes() {
        let temp = user_temp_dir().unwrap();
        assert!(
            Path::new(&temp).is_dir() && can_write(Path::new(&temp)),
            "{temp}"
        );
        // The data volume is hidden from Finder itself: only /Volumes
        // entries are judged by that.
        let data = mount_info(Path::new("/System/Volumes/Data")).unwrap();
        assert!(data.local && data.file_system == "apfs", "{data:?}");
    }

    #[test]
    fn the_temporary_folder_is_writable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(can_write(dir.path()));
        assert!(!can_write(Path::new("/no/such/folder")));
    }
}
