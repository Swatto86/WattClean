//! This Linux or macOS machine as the signed-in account sees it.

use std::collections::HashSet;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wcc_core::system::{Drive, ProgramOutput, Registry, System};

use crate::folders::Folders;
use crate::protected::{HOME_FOLDERS, SYSTEM_FOLDERS};
use crate::{drives, process, sys, trash};

/// How long a clipboard or process query may take.
const QUICK: Duration = Duration::from_secs(3);

pub struct UnixSystem {
    folders: Folders,
}

impl UnixSystem {
    pub fn new() -> Self {
        Self {
            folders: Folders::of_this_account(),
        }
    }

    fn home(&self) -> Option<PathBuf> {
        self.folders.path("HOME")
    }

    fn tool(&self, name: &str) -> Option<PathBuf> {
        process::find(name, self.home().as_deref())
    }

    #[cfg(not(target_os = "macos"))]
    fn trash_folders(&self) -> Vec<PathBuf> {
        let roots: Vec<String> = drives::drives()
            .into_iter()
            .map(|drive| drive.root)
            .collect();
        trash::linux_folders(self.folders.path("TRASH").as_deref(), &roots, sys::uid())
    }
}

impl Default for UnixSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Linux and macOS have no registry: nothing is found, nothing removed.
struct NoRegistry;

impl Registry for NoRegistry {
    fn key_exists(&self, _key: &str) -> bool {
        false
    }
    fn value_exists(&self, _key: &str, _name: &str) -> bool {
        false
    }
    fn subkeys(&self, _key: &str) -> Vec<String> {
        Vec::new()
    }
    fn delete_key(&self, _key: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "there is no registry here",
        ))
    }
    fn delete_value(&self, _key: &str, _name: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "there is no registry here",
        ))
    }
}

/// Names of running programs from `/proc`: each one's short name and the
/// file name it was started as, both in lower case.
#[cfg(not(target_os = "macos"))]
fn running_programs() -> HashSet<String> {
    let mut names = HashSet::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return names;
    };
    for entry in entries.flatten() {
        let is_process = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()));
        if !is_process {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
            names.insert(comm.trim().to_lowercase());
        }
        if let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) {
            let first = cmdline.split(|b| *b == 0).next().unwrap_or_default();
            let first = String::from_utf8_lossy(first);
            if let Some(name) = Path::new(first.as_ref()).file_name() {
                names.insert(name.to_string_lossy().to_lowercase());
            }
        }
    }
    names.remove("");
    names
}

#[cfg(target_os = "macos")]
fn running_programs() -> HashSet<String> {
    process::quick(Path::new("/bin/ps"), &["-A", "-c", "-o", "comm="], QUICK)
        .unwrap_or_default()
        .lines()
        .map(|line| line.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

impl System for UnixSystem {
    fn var(&self, name: &str) -> Vec<String> {
        self.folders
            .get(name)
            .map(str::to_string)
            .into_iter()
            .collect()
    }

    fn registry(&self) -> &dyn Registry {
        &NoRegistry
    }

    #[cfg(not(target_os = "macos"))]
    fn recycle_bin(&self) -> io::Result<(u64, u64)> {
        trash::count(&self.trash_folders())
    }

    /// The Trash folder itself when macOS lets this program read it (Full
    /// Disk Access), else Finder's count, without sizes.
    #[cfg(target_os = "macos")]
    fn recycle_bin(&self) -> io::Result<(u64, u64)> {
        let folder = self
            .folders
            .path("TRASH")
            .ok_or_else(|| io::Error::other("no home folder"))?;
        match std::fs::read_dir(&folder) {
            Ok(entries) => {
                let mut items = 0;
                let mut bytes = 0;
                for entry in entries.flatten() {
                    if entry.file_name() == ".DS_Store" {
                        continue;
                    }
                    items += 1;
                    bytes += wcc_core::command::measure(self, &[&entry.path().to_string_lossy()]);
                }
                Ok((items, bytes))
            }
            Err(_) => {
                let count = trash::finder("tell application \"Finder\" to count items of trash")?;
                let items = count
                    .parse()
                    .map_err(|_| io::Error::other(format!("Finder said {count:?}")))?;
                Ok((items, 0))
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn empty_recycle_bin(&self) -> io::Result<()> {
        trash::empty(&self.trash_folders())
    }

    #[cfg(target_os = "macos")]
    fn empty_recycle_bin(&self) -> io::Result<()> {
        trash::finder("tell application \"Finder\" to empty trash").map(|_| ())
    }

    fn clipboard_has_data(&self) -> bool {
        let read = |name: &str, args: &[&str]| {
            self.tool(name)
                .and_then(|tool| process::quick(&tool, args, QUICK))
                .is_some_and(|text| !text.trim().is_empty())
        };
        if cfg!(target_os = "macos") {
            return read("pbpaste", &[]);
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && self.tool("wl-paste").is_some() {
            return read("wl-paste", &["--list-types"]);
        }
        read("xclip", &["-selection", "clipboard", "-o"])
            || read("xsel", &["--clipboard", "--output"])
    }

    fn clear_clipboard(&self) -> io::Result<()> {
        let (name, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
            ("pbcopy", &[])
        } else if std::env::var_os("WAYLAND_DISPLAY").is_some() && self.tool("wl-copy").is_some() {
            ("wl-copy", &["--clear"])
        } else if self.tool("xsel").is_some() {
            ("xsel", &["--clipboard", "--clear"])
        } else {
            ("xclip", &["-selection", "clipboard", "-i", "/dev/null"])
        };
        let tool = self.tool(name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "clearing the clipboard needs wl-clipboard, xsel or xclip installed",
            )
        })?;
        // pbcopy reads nothing from an empty input and so empties the clipboard.
        let status = Command::new(tool)
            .args(args)
            .stdin(Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{name} could not clear the clipboard"
            )))
        }
    }

    fn running(&self) -> HashSet<String> {
        running_programs()
    }

    fn is_elevated(&self) -> bool {
        sys::is_elevated()
    }

    /// Volumes nothing on them lets this account write to cannot be wiped.
    fn drives(&self) -> Vec<Drive> {
        drives::drives()
            .into_iter()
            .filter(|drive| sys::can_write(&self.wipe_folder(drive)))
            .collect()
    }

    /// The drive with the same device as `path`; else by path (a
    /// memory-backed `/tmp` belongs to the root's drive, where the wipe then
    /// picks a folder on the real disk).
    fn drive_of(&self, path: &Path) -> Option<Drive> {
        let device = |p: &Path| std::fs::metadata(p).ok().map(|meta| meta.dev());
        let drives = self.drives();
        let wanted = device(path);
        drives
            .iter()
            .find(|drive| wanted.is_some() && device(Path::new(&drive.root)) == wanted)
            .cloned()
            .or_else(|| wcc_core::system::drive_holding(drives, path))
    }

    fn free_space(&self, path: &Path) -> io::Result<u64> {
        sys::space(path).map(|space| space.free)
    }

    fn delete_on_reboot(&self, _path: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Linux and macOS delete a file in use at once: this one could not be deleted",
        ))
    }

    fn link_count(&self, path: &Path) -> io::Result<u32> {
        let links = std::fs::symlink_metadata(path)?.nlink();
        Ok(u32::try_from(links).unwrap_or(u32::MAX))
    }

    fn wipe_folder(&self, drive: &Drive) -> PathBuf {
        let mut candidates: Vec<PathBuf> = self.folders.path("TEMP").into_iter().collect();
        candidates.push(PathBuf::from("/var/tmp"));
        candidates.extend(self.home());
        drives::wipe_folder(drive, &candidates)
    }

    fn protected_folders(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = SYSTEM_FOLDERS.iter().map(PathBuf::from).collect();
        out.extend(
            wcc_core::vars::UNIX_VARIABLES
                .iter()
                .filter_map(|name| self.folders.path(name)),
        );
        if let Some(home) = self.home() {
            out.extend(HOME_FOLDERS.iter().map(|rel| home.join(rel)));
        }
        out
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        self.tool(name)
    }

    fn run_program(
        &self,
        program: &Path,
        args: &[String],
        admin: bool,
    ) -> io::Result<ProgramOutput> {
        process::run(program, args, admin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_folder_and_the_root_are_protected() {
        let system = UnixSystem::new();
        let guard = wcc_core::paths::Guard::new(&system.protected_folders(), &[]);
        let home = system.home().unwrap();
        assert!(guard.check(&home).is_err());
        assert!(guard.check(Path::new("/")).is_err());
        assert!(guard.check(Path::new("/usr/local")).is_err());
        assert!(guard.check(&home.join(".local/share")).is_err());
        assert!(guard.check(&home.join(".cache/some-app")).is_ok());
    }

    #[test]
    fn this_program_is_among_the_running_ones() {
        let names = UnixSystem::new().running();
        assert!(!names.is_empty());
        let me = std::env::current_exe().unwrap();
        let me = me.file_name().unwrap().to_string_lossy().to_lowercase();
        let short: String = me.chars().take(15).collect();
        assert!(
            names.contains(&me) || names.contains(&short),
            "{me} in {names:?}"
        );
    }

    #[test]
    fn every_definition_variable_has_a_folder() {
        let system = UnixSystem::new();
        for name in wcc_core::vars::UNIX_VARIABLES {
            assert_eq!(system.var(name).len(), 1, "{name}");
        }
    }
}
