//! The uninstaller on Linux and macOS: what the package managers installed,
//! removing it with the package manager's own command, and the folders it
//! left in the home folder, which go to the Trash.
//!
//! Windows has no package manager to ask; its uninstaller reads the
//! registry instead (the rest of this crate).

mod daemons;
mod extra;
mod keys;
mod launchd;
mod launchers;
mod leftovers;
mod linux;
mod macos;
mod orphans;
mod others;
mod remove;

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub use keys::keys_for;
pub use launchd::{Domain, Job, LaunchdJob};
pub use leftovers::{Leftover, find_leftovers};
pub use orphans::{Orphans, orphans};
pub use remove::{Removal, Step, removal, stop_job};

/// What installed a package, and so what removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Manager {
    /// Debian, Ubuntu and their family (removed with apt-get).
    Apt,
    /// Fedora, RHEL, openSUSE (removed with dnf, zypper or rpm).
    Rpm,
    Pacman,
    Flatpak,
    Snap,
    /// A single AppImage file.
    AppImage,
    Homebrew,
    HomebrewCask,
    MacPorts,
    /// An application bundle in an Applications folder on macOS.
    MacApp,
}

impl Manager {
    pub fn label(self) -> &'static str {
        match self {
            Self::Apt => "APT",
            Self::Rpm => "RPM",
            Self::Pacman => "pacman",
            Self::Flatpak => "Flatpak",
            Self::Snap => "Snap",
            Self::AppImage => "AppImage",
            Self::Homebrew => "Homebrew",
            Self::HomebrewCask => "Homebrew Cask",
            Self::MacPorts => "MacPorts",
            Self::MacApp => "Application",
        }
    }
}

/// An installed package or application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub manager: Manager,
    /// The package manager's own name for it (a package name, a Flatpak
    /// application id, an AppImage's or bundle's path).
    pub id: String,
    /// The name people know it by.
    pub name: String,
    pub version: Option<String>,
    pub size: Option<u64>,
    pub summary: Option<String>,
    /// Something people start (it has a launcher or is an app bundle),
    /// rather than a library or tool.
    pub app: bool,
    /// macOS bundle identifier (`com.vendor.App`), for leftovers.
    pub bundle_id: Option<String>,
    /// Flatpak installations: `user` or `system`.
    pub installation: Option<String>,
    /// A cask's own list of what to trash when it is zapped (`~/Library/…`).
    #[serde(skip)]
    pub zap: Vec<String>,
    /// The file names (lower case) of the app bundles a cask installs.
    #[serde(skip)]
    pub app_files: Vec<String>,
    /// Other names it goes by: a cask's app's own name.
    #[serde(skip)]
    pub known_as: Vec<String>,
    /// The app bundle a cask installed, once it was found.
    #[serde(skip)]
    pub app_path: Option<PathBuf>,
}

impl Package {
    pub fn new(manager: Manager, id: &str, name: &str) -> Self {
        Self {
            manager,
            id: id.to_string(),
            name: name.to_string(),
            version: None,
            size: None,
            summary: None,
            app: false,
            bundle_id: None,
            installation: None,
            zap: Vec::new(),
            app_files: Vec::new(),
            known_as: Vec::new(),
            app_path: None,
        }
    }

    /// The app bundle that goes with it: an app's own, or the one a cask
    /// installed.
    pub fn bundle(&self) -> Option<&Path> {
        match self.manager {
            Manager::MacApp => Some(Path::new(&self.id)),
            _ => self.app_path.as_deref(),
        }
    }

    /// The key a page names a package by: its manager and id, and for a
    /// Flatpak its installation, since one app may be installed both for
    /// the account and for everyone.
    pub fn key(&self) -> String {
        match (self.manager, &self.installation) {
            (Manager::Flatpak, Some(installation)) => {
                format!("{}:{installation}:{}", self.manager.label(), self.id)
            }
            _ => format!("{}:{}", self.manager.label(), self.id),
        }
    }
}

/// What a command printed and whether it succeeded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// What the program said last, for a message: its last error line on
    /// stderr (`Error: …`, `E: …`), else its last line there, else its last
    /// line on stdout, where package managers print their progress. When
    /// Homebrew's error only introduces the output of a step that failed
    /// ("… Here's the output:"), the first line of that output is the cause.
    /// pacman's `error:` on stderr is general ("could not satisfy
    /// dependencies"); the reason is its last `:: ` line on stdout.
    pub fn last_words(&self) -> Option<&str> {
        fn said(text: &str) -> impl DoubleEndedIterator<Item = &str> {
            text.lines().map(str::trim).filter(|line| !line.is_empty())
        }
        let errors: Vec<&str> = said(&self.stderr).collect();
        if errors.iter().any(|line| line.starts_with("error:"))
            && let Some(reason) = said(&self.stdout)
                .rev()
                .find_map(|line| line.strip_prefix(":: "))
        {
            return Some(reason);
        }
        let error = errors
            .iter()
            .rposition(|line| line.starts_with("Error:") || line.starts_with("E:"));
        if let Some(at) = error {
            let line = errors.get(at).copied();
            let cause = errors.get(at + 1).copied();
            return match line {
                Some(line) if line.ends_with("Here's the output:") => cause.or(Some(line)),
                _ => line,
            };
        }
        said(&self.stderr)
            .next_back()
            .or_else(|| said(&self.stdout).next_back())
    }
}

/// An application bundle's own description (its `Info.plist`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundleInfo {
    pub bundle_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
}

/// Everything the package engine needs from the machine.
pub trait PackageSystem: Send + Sync {
    fn is_macos(&self) -> bool;
    fn home(&self) -> Option<PathBuf>;
    /// A path of the machine as this system sees it: itself on a real
    /// machine, inside the sandbox on a test one.
    fn at(&self, path: &str) -> PathBuf {
        PathBuf::from(path)
    }
    /// A program in the system's own program folders.
    fn find_program(&self, name: &str) -> Option<PathBuf>;
    /// A program that will run with administrator rights: only from
    /// folders only an administrator can change, never the account's own
    /// (Homebrew's, the home folder, `PATH`).
    fn find_admin_program(&self, name: &str) -> Option<PathBuf>;
    /// Runs a program (never through a shell); `admin` asks for
    /// administrator rights first. `env` sets variables for this one run;
    /// a program run through the administrator prompt cannot be given any.
    fn run(
        &self,
        program: &Path,
        args: &[String],
        admin: bool,
        env: &[(&str, &str)],
    ) -> io::Result<Output>;
    /// An application bundle's `Info.plist`.
    fn bundle_info(&self, bundle: &Path) -> Option<BundleInfo>;
    /// Moves files and folders to the Trash, one result each.
    fn trash(&self, paths: &[PathBuf]) -> Vec<io::Result<()>>;
    /// The full paths of the programs running from inside `folder` (an app
    /// bundle, see [`within`]); nothing where that cannot be told.
    fn running_from(&self, _folder: &Path) -> Vec<String> {
        Vec::new()
    }
    /// What a launchd job's plist says, when it can be read.
    fn launchd_job(&self, _plist: &Path) -> Option<LaunchdJob> {
        None
    }
    /// This account's user id: launchd's `gui/<uid>` session.
    fn uid(&self) -> Option<u32> {
        None
    }
    /// Shows a file or folder in the file manager: Finder selects it on
    /// macOS; on Linux the folder holding it opens.
    fn reveal(&self, path: &Path) -> io::Result<()>;
    /// The program sudo asks for an administrator's password through
    /// (`SUDO_ASKPASS`) when Homebrew needs one on macOS, written afresh.
    fn askpass(&self) -> io::Result<PathBuf> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "there is no password box for Homebrew here",
        ))
    }
}

/// Whether `path` lies inside `folder`: a whole-name prefix ending in `/`,
/// so `Foo.app` never holds `Foo.app2/…`. Letter case is ignored, as on
/// macOS's own disks.
pub fn within(path: &str, folder: &Path) -> bool {
    let folder = folder.to_string_lossy();
    let folder = folder.trim_end_matches('/');
    let Some((head, rest)) = path.split_at_checked(folder.len()) else {
        return false;
    };
    !folder.is_empty()
        && head.eq_ignore_ascii_case(folder)
        && rest.len() > 1
        && rest.starts_with('/')
}

/// Runs a read-only listing; its output, or nothing when the program is
/// missing or fails.
pub(crate) fn query(system: &dyn PackageSystem, program: &str, args: &[&str]) -> Option<String> {
    let program = system.find_program(program)?;
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    system
        .run(&program, &args, false, &[])
        .ok()
        .filter(|output| output.success)
        .map(|output| output.stdout)
}

/// Everything installed, applications first then by name; with `all`
/// false, applications only. Each package manager that is not installed
/// or fails is left out; the second value names the ones that failed.
pub fn list(system: &dyn PackageSystem, all: bool) -> (Vec<Package>, Vec<String>) {
    let (mut packages, problems) = if system.is_macos() {
        macos::list(system)
    } else {
        linux::list(system)
    };
    if !all {
        packages.retain(|package| package.app);
    }
    packages
        .sort_by_cached_key(|package| (!package.app, package.name.to_lowercase(), package.key()));
    packages.dedup_by(|a, b| a.key() == b.key());
    (packages, problems)
}

/// Bytes in a file or folder; links are counted, never followed.
pub(crate) fn tree_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().map(|e| tree_size(&e.path())).sum())
        .unwrap_or(0)
}

/// `1.2 GB`, `640 kB`, `12 MiB` and the like, as package managers print
/// sizes, in bytes.
pub(crate) fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim().replace(',', ".");
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    let factor: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" | "bytes" => 1.0,
        "k" | "kb" => 1e3,
        "kib" => 1024.0,
        "m" | "mb" => 1e6,
        "mib" => 1_048_576.0,
        "g" | "gb" => 1e9,
        "gib" => 1_073_741_824.0,
        "t" | "tb" => 1e12,
        "tib" => 1_099_511_627_776.0,
        _ => return None,
    };
    let bytes = (number * factor).round();
    // A size is never negative and fits u64 long before f64 loses it.
    (0.0..1.8e19)
        .contains(&bytes)
        .then(|| format!("{bytes:.0}").parse().ok())
        .flatten()
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
