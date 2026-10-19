//! Whether a path registry data names is really gone. Only a confident
//! "missing" lets a scan offer an entry; anything unclear counts as present.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::paths::{expand, fold, is_absolute_local, is_within};
use crate::system::{DriveKind, System};

/// What a check concluded about one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Present,
    Missing,
    /// Not checked: a bare name, a network share, an unplugged drive, or a
    /// Windows path the person chose to leave alone.
    Unknown,
}

/// Variables whose value is different for each account.
const PER_USER: [&str; 11] = [
    "%appdata%",
    "%localappdata%",
    "%userprofile%",
    "%homepath%",
    "%homedrive%",
    "%temp%",
    "%tmp%",
    "%username%",
    "%onedrive%",
    "%onedriveconsumer%",
    "%onedrivecommercial%",
];

pub struct PathCheck<'a> {
    system: &'a dyn System,
    windows: Option<PathBuf>,
    exclude_removable: bool,
    exclude_windows: bool,
    /// False for another account's data: its per-user variables are not
    /// this account's, so a path using one cannot be judged from here.
    own_account: bool,
    drives: Mutex<HashMap<char, DriveKind>>,
}

impl<'a> PathCheck<'a> {
    pub fn new(system: &'a dyn System, exclude_removable: bool, exclude_windows: bool) -> Self {
        let windows = system
            .env_var("SystemRoot")
            .or_else(|| system.env_var("windir"))
            .map(PathBuf::from);
        Self {
            system,
            windows,
            exclude_removable,
            exclude_windows,
            own_account: true,
            drives: Mutex::new(HashMap::new()),
        }
    }

    /// The same check for other accounts' data.
    pub fn for_other_accounts(mut self) -> Self {
        self.own_account = false;
        self
    }

    /// Check a path as written in registry data (quotes and variables
    /// allowed). `view32` is true for data in the 32-bit registry, where
    /// `System32` means `SysWOW64`.
    pub fn path(&self, raw: &str, view32: bool) -> Verdict {
        if !self.own_account {
            let lower = raw.to_lowercase();
            if PER_USER.iter().any(|variable| lower.contains(variable)) {
                return Verdict::Unknown;
            }
        }
        let expanded = expand(self.system, raw.trim().trim_matches('"'));
        let text = expanded.trim();
        if !is_absolute_local(text) || text.contains(['*', '?', '<', '>', '|']) {
            return Verdict::Unknown;
        }
        let path = PathBuf::from(text);
        if self.exclude_windows
            && self
                .windows
                .as_deref()
                .is_some_and(|windows| is_within(&path, windows))
        {
            return Verdict::Unknown;
        }
        match self.drive(&path) {
            DriveKind::Fixed => {}
            DriveKind::Network => return Verdict::Unknown,
            DriveKind::Removable | DriveKind::Missing if self.exclude_removable => {
                return Verdict::Unknown;
            }
            DriveKind::Missing => return Verdict::Missing,
            DriveKind::Removable => {}
        }
        let real = if view32 {
            self.redirect_32(&path)
        } else {
            path
        };
        // Only "not found" means gone: a folder Windows will not let this
        // account look into (WindowsApps, another account's profile) proves
        // nothing.
        match std::fs::metadata(&real) {
            Ok(_) => Verdict::Present,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Verdict::Missing,
            Err(_) => Verdict::Unknown,
        }
    }

    pub fn missing(&self, raw: &str, view32: bool) -> bool {
        self.path(raw, view32) == Verdict::Missing
    }

    fn drive(&self, path: &Path) -> DriveKind {
        let letter = path
            .to_string_lossy()
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or('?');
        let mut drives = self
            .drives
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *drives
            .entry(letter)
            .or_insert_with(|| self.system.drive_kind(path))
    }

    /// `C:\Windows\System32\x` in 32-bit data is `C:\Windows\SysWOW64\x`.
    fn redirect_32(&self, path: &Path) -> PathBuf {
        let Some(windows) = &self.windows else {
            return path.to_path_buf();
        };
        let system32 = windows.join("System32");
        if !is_within(path, &system32) {
            return path.to_path_buf();
        }
        let rest_len = fold(&system32).len();
        let text = path.to_string_lossy();
        let rest = text
            .get(rest_len..)
            .unwrap_or_default()
            .trim_start_matches('\\');
        windows.join("SysWOW64").join(rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeSystem;

    fn machine() -> (tempfile::TempDir, FakeSystem) {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::empty(dir.path());
        let windows = dir.path().join("Windows");
        std::fs::create_dir_all(windows.join("System32")).unwrap();
        std::fs::create_dir_all(windows.join("SysWOW64")).unwrap();
        std::fs::write(windows.join(r"SysWOW64\only32.dll"), b"x").unwrap();
        std::fs::write(dir.path().join("here.exe"), b"x").unwrap();
        system.set_env("SystemRoot", &windows.to_string_lossy());
        (dir, system)
    }

    #[test]
    fn present_missing_and_unknown() {
        let (dir, system) = machine();
        let check = PathCheck::new(&system, true, false);
        let here = dir.path().join("here.exe");
        let gone = dir.path().join("gone.exe");
        assert_eq!(check.path(&here.to_string_lossy(), false), Verdict::Present);
        assert_eq!(
            check.path(&format!("\"{}\"", gone.display()), false),
            Verdict::Missing
        );
        assert_eq!(check.path("notepad.exe", false), Verdict::Unknown);
        assert_eq!(check.path(r"\\server\share\a.exe", false), Verdict::Unknown);
        assert_eq!(check.path(r"Q:\Portable\a.exe", false), Verdict::Unknown);
        let keep_drives = PathCheck::new(&system, false, false);
        assert_eq!(
            keep_drives.path(r"Q:\Portable\a.exe", false),
            Verdict::Missing
        );
    }

    #[test]
    fn windows_paths_can_be_left_alone() {
        let (_dir, system) = machine();
        let gone = r"%SystemRoot%\System32\gone.dll";
        assert_eq!(
            PathCheck::new(&system, true, true).path(gone, false),
            Verdict::Unknown
        );
        assert_eq!(
            PathCheck::new(&system, true, false).path(gone, false),
            Verdict::Missing
        );
    }

    #[test]
    fn thirty_two_bit_data_reads_system32_as_syswow64() {
        let (_dir, system) = machine();
        let check = PathCheck::new(&system, true, false);
        let path = r"%SystemRoot%\System32\only32.dll";
        assert_eq!(check.path(path, true), Verdict::Present);
        assert_eq!(check.path(path, false), Verdict::Missing);
    }
}
