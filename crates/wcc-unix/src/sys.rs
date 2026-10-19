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

    #[test]
    fn the_temporary_folder_is_writable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(can_write(dir.path()));
        assert!(!can_write(Path::new("/no/such/folder")));
    }
}
