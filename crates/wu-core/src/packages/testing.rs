//! A machine for the package engine's tests: a home folder and a system
//! root in a temporary folder, programs that print what a test gives them,
//! and a record of every command run.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::{BundleInfo, Output, Package, PackageSystem, find_leftovers};

pub struct Machine {
    dir: tempfile::TempDir,
    macos: bool,
    /// Program name → what it prints, whatever its arguments.
    pub prints: Mutex<BTreeMap<String, String>>,
    /// Every command run, `admin ` first when it asked for rights.
    pub ran: Mutex<Vec<String>>,
    pub bundles: BTreeMap<PathBuf, BundleInfo>,
    /// Programs only in a folder the account can write: never found for a
    /// command run as administrator.
    pub untrusted: BTreeSet<String>,
    /// Where a program is found, when not in `/usr/bin`.
    pub places: BTreeMap<String, PathBuf>,
}

impl Machine {
    pub fn new(programs: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        Self {
            dir,
            macos: false,
            prints: Mutex::new(
                programs
                    .iter()
                    .map(|p| ((*p).to_string(), String::new()))
                    .collect(),
            ),
            ran: Mutex::new(Vec::new()),
            bundles: BTreeMap::new(),
            untrusted: BTreeSet::new(),
            places: BTreeMap::new(),
        }
    }

    pub fn macos() -> Self {
        Self {
            macos: true,
            ..Self::new(&[])
        }
    }

    pub fn prints(&self, program: &str, text: &str) {
        self.prints
            .lock()
            .unwrap()
            .insert(program.to_string(), text.to_string());
    }

    pub fn ran(&self) -> Vec<String> {
        self.ran.lock().unwrap().clone()
    }
}

impl PackageSystem for Machine {
    fn is_macos(&self) -> bool {
        self.macos
    }

    fn home(&self) -> Option<PathBuf> {
        Some(self.dir.path().join("home"))
    }

    fn at(&self, path: &str) -> PathBuf {
        self.dir
            .path()
            .join("root")
            .join(path.trim_start_matches('/'))
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        self.prints.lock().unwrap().contains_key(name).then(|| {
            self.places
                .get(name)
                .cloned()
                .unwrap_or_else(|| PathBuf::from("/usr/bin").join(name))
        })
    }

    fn find_admin_program(&self, name: &str) -> Option<PathBuf> {
        self.find_program(name)
            .filter(|_| !self.untrusted.contains(name))
    }

    fn run(&self, program: &Path, args: &[String], admin: bool) -> io::Result<Output> {
        let name = program.file_name().unwrap().to_string_lossy().to_string();
        let stdout = self
            .prints
            .lock()
            .unwrap()
            .get(&name)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let mut line = if admin { format!("admin {name}") } else { name };
        for arg in args {
            line.push(' ');
            line.push_str(arg);
        }
        self.ran.lock().unwrap().push(line);
        Ok(Output {
            success: true,
            stdout,
            stderr: String::new(),
        })
    }

    fn bundle_info(&self, bundle: &Path) -> Option<BundleInfo> {
        self.bundles.get(bundle).cloned()
    }

    fn trash(&self, paths: &[PathBuf]) -> Vec<io::Result<()>> {
        let bin = self.dir.path().join("trash");
        paths
            .iter()
            .map(|path| {
                std::fs::create_dir_all(&bin)?;
                let name = path
                    .file_name()
                    .ok_or_else(|| io::Error::other("no name"))?;
                std::fs::rename(path, bin.join(name))
            })
            .collect()
    }
}

/// The paths found, relative to the home folder.
pub fn offered(machine: &Machine, removed: &Package, installed: &[Package]) -> Vec<String> {
    let home = machine.home().unwrap();
    find_leftovers(machine, removed, installed)
        .iter()
        .map(|l| {
            l.path
                .strip_prefix(&home)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}
