//! What the engine needs from the machine beyond plain files: variables and
//! known folders, the registry, the Recycle Bin, the clipboard, running
//! programs, drives and rights. `wcc-windows` implements it for Windows;
//! `fake` implements it over a sandbox folder for tests.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The operating system a machine runs: it decides which built-in cleaners
/// apply and what the window calls things.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
    Macos,
}

impl Platform {
    /// The platform this program was built for.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

/// What a program a task ran printed and whether it succeeded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgramOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait System: Send + Sync {
    /// Which built-in cleaners apply to this machine.
    fn platform(&self) -> Platform {
        Platform::current()
    }
    /// The full path of a program a task runs (a package manager), from
    /// the system's own program folders; `None` when it is not installed.
    fn find_program(&self, _name: &str) -> Option<PathBuf> {
        None
    }
    /// The same for a program that will run with administrator rights:
    /// only from folders only an administrator can change, never the
    /// account's own (Homebrew's, the home folder, `PATH`), where anything
    /// the account runs could have put a program to be run as root.
    fn find_admin_program(&self, _name: &str) -> Option<PathBuf> {
        None
    }
    /// Runs a program with these arguments (never through a shell) and
    /// waits for it; `admin` asks for administrator (root) rights first
    /// when this program does not have them. `env` sets variables for this
    /// one run.
    fn run_program(
        &self,
        _program: &Path,
        _args: &[String],
        _admin: bool,
        _env: &[(&str, &str)],
    ) -> io::Result<ProgramOutput> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this machine cannot run maintenance commands",
        ))
    }
    /// The folders (or text) a `%VARIABLE%` stands for; empty when unknown.
    fn var(&self, name: &str) -> Vec<String>;
    fn registry(&self) -> &dyn Registry;
    /// Items and bytes in the Recycle Bin across every drive.
    fn recycle_bin(&self) -> io::Result<(u64, u64)>;
    fn empty_recycle_bin(&self) -> io::Result<()>;
    fn clipboard_has_data(&self) -> bool;
    fn clear_clipboard(&self) -> io::Result<()>;
    /// Executable names (lower case) of running programs.
    fn running(&self) -> HashSet<String>;
    fn is_elevated(&self) -> bool;
    /// Fixed and removable drives, for wiping free space.
    fn drives(&self) -> Vec<Drive>;
    /// Whether the drive's file system compresses what is written (Btrfs
    /// or bcachefs mounted with compression, on Linux): a wipe's zeros
    /// would take a sliver of their size there, so it is never wiped.
    fn compresses(&self, _drive: &Drive) -> bool {
        false
    }
    /// Bytes free to this user on the drive holding `path`.
    fn free_space(&self, path: &Path) -> io::Result<u64>;
    /// Asks Windows to delete a file in use at the next restart.
    fn delete_on_reboot(&self, path: &Path) -> io::Result<()>;
    /// How many names (hard links) a file's data has; shredding refuses
    /// more than one.
    fn link_count(&self, path: &Path) -> io::Result<u32>;
    /// Tells Explorer that recent-file lists changed.
    fn shell_changed(&self) {}
    /// How many bytes a free-space wipe may write before it stops; `None`
    /// fills the drive. The test machine caps it.
    fn wipe_limit(&self) -> Option<u64> {
        None
    }

    /// The drive holding `path`.
    fn drive_of(&self, path: &Path) -> Option<Drive> {
        drive_holding(self.drives(), path)
    }

    /// Where a free-space wipe writes on a drive: the temporary folder when
    /// it is on that drive (no rights needed), else the drive's root.
    fn wipe_folder(&self, drive: &Drive) -> PathBuf {
        let root = crate::paths::key(Path::new(&drive.root));
        self.var("TEMP")
            .into_iter()
            .find(|temp| crate::paths::is_within(&crate::paths::key(Path::new(temp)), &root))
            .map_or_else(|| PathBuf::from(&drive.root), PathBuf::from)
    }

    /// Folders no cleaner may remove (they may be emptied).
    fn protected_folders(&self) -> Vec<PathBuf> {
        const NAMES: &[&str] = &[
            "WINDIR",
            "SYSTEMROOT",
            "SYSTEMDRIVE",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "LOCALLOWAPPDATA",
            "TEMP",
            "TMP",
            "PROGRAMDATA",
            "PROGRAMFILES",
            "COMMONPROGRAMFILES",
            "PUBLIC",
            "DOCUMENTS",
            "DESKTOP",
            "DOWNLOADS",
            "PICTURES",
            "MUSIC",
            "VIDEO",
            "FAVORITES",
            "STARTMENU",
            "ONEDRIVE",
        ];
        let mut out: Vec<PathBuf> = NAMES
            .iter()
            .flat_map(|name| self.var(name))
            .map(PathBuf::from)
            .collect();
        for windir in self.var("WINDIR") {
            for sub in [
                "System32",
                "SysWOW64",
                "WinSxS",
                "Fonts",
                "Installer",
                "assembly",
            ] {
                out.push(Path::new(&windir).join(sub));
            }
        }
        out
    }
}

/// Registry keys are written `HKCU\Software\…` (or the long hive names).
pub trait Registry: Send + Sync {
    fn key_exists(&self, key: &str) -> bool;
    fn value_exists(&self, key: &str, name: &str) -> bool;
    /// The names of the key's direct subkeys; empty when there are none or
    /// the key cannot be read.
    fn subkeys(&self, key: &str) -> Vec<String>;
    /// Deletes the key with everything below it.
    fn delete_key(&self, key: &str) -> io::Result<()>;
    fn delete_value(&self, key: &str, name: &str) -> io::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    /// `C:\`
    pub root: String,
    pub label: String,
    pub file_system: String,
    pub free: u64,
    pub total: u64,
    pub removable: bool,
}

/// Why a drive that compresses is not wiped.
pub const COMPRESSES: &str = "this drive compresses what is written, so a wipe's zeros would take almost no space and could not overwrite its free space";

/// The first drive whose root holds `path`, by the text of their paths.
pub fn drive_holding(drives: Vec<Drive>, path: &Path) -> Option<Drive> {
    let key = crate::paths::key(path);
    drives
        .into_iter()
        .find(|d| crate::paths::is_within(&key, &crate::paths::key(Path::new(&d.root))))
}

/// The hive and the rest of a registry key, with the hive in short form.
pub fn split_hive(key: &str) -> Option<(&'static str, &str)> {
    let (hive, rest) = key.split_once('\\').unwrap_or((key, ""));
    let short = match hive.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => "HKCU",
        "HKLM" | "HKEY_LOCAL_MACHINE" => "HKLM",
        "HKCR" | "HKEY_CLASSES_ROOT" => "HKCR",
        "HKU" | "HKEY_USERS" => "HKU",
        _ => return None,
    };
    Some((short, rest.trim_matches('\\')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hives_take_either_name() {
        assert_eq!(
            split_hive(r"HKEY_CURRENT_USER\Software\X\"),
            Some(("HKCU", r"Software\X"))
        );
        assert_eq!(split_hive(r"hklm\Software"), Some(("HKLM", "Software")));
        assert_eq!(split_hive("HKCU"), Some(("HKCU", "")));
        assert_eq!(split_hive(r"HKXX\Software"), None);
    }
}
