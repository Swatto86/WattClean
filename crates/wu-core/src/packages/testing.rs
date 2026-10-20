//! A machine for the package engine's tests: a home folder and a system
//! root in a temporary folder, programs that print what a test gives them,
//! and a record of every command run.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::{
    BundleInfo, Job, LaunchdJob, Leftover, Output, Package, PackageSystem, find_leftovers, within,
};

pub struct Machine {
    dir: tempfile::TempDir,
    macos: bool,
    /// Program name → what it prints, whatever its arguments.
    pub prints: Mutex<BTreeMap<String, String>>,
    /// A whole command (`rpm -e --test vlc`) → its answer, before `prints`.
    pub replies: Mutex<BTreeMap<String, Output>>,
    /// Every command run, `admin ` first when it asked for rights.
    pub ran: Mutex<Vec<String>>,
    pub bundles: BTreeMap<PathBuf, BundleInfo>,
    /// Programs only in a folder the account can write: never found for a
    /// command run as administrator.
    pub untrusted: BTreeSet<String>,
    /// Where a program is found, when not in `/usr/bin`.
    pub places: BTreeMap<String, PathBuf>,
    /// The full paths of the programs running.
    pub running: Vec<String>,
    /// What each launchd plist says.
    pub jobs: BTreeMap<PathBuf, LaunchdJob>,
    /// Every path shown in the file manager.
    pub revealed: Mutex<Vec<PathBuf>>,
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
            replies: Mutex::new(BTreeMap::new()),
            ran: Mutex::new(Vec::new()),
            bundles: BTreeMap::new(),
            untrusted: BTreeSet::new(),
            places: BTreeMap::new(),
            running: Vec::new(),
            jobs: BTreeMap::new(),
            revealed: Mutex::new(Vec::new()),
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

    /// What one whole command answers, as a real program would: `success`
    /// false when `stderr` holds an error.
    pub fn replies(&self, command: &str, success: bool, stdout: &str, stderr: &str) {
        self.replies.lock().unwrap().insert(
            command.to_string(),
            Output {
                success,
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
            },
        );
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

    fn run(
        &self,
        program: &Path,
        args: &[String],
        admin: bool,
        env: &[(&str, &str)],
    ) -> io::Result<Output> {
        let name = program.file_name().unwrap().to_string_lossy().to_string();
        let stdout = self
            .prints
            .lock()
            .unwrap()
            .get(&name)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let mut command = name;
        for arg in args {
            command.push(' ');
            command.push_str(arg);
        }
        let reply = self.replies.lock().unwrap().get(&command).cloned();
        let mut line = if admin {
            format!("admin {command}")
        } else {
            command
        };
        for (name, value) in env.iter().rev() {
            line = format!("{name}={value} {line}");
        }
        self.ran.lock().unwrap().push(line);
        Ok(reply.unwrap_or(Output {
            success: true,
            stdout,
            stderr: String::new(),
        }))
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

    fn running_from(&self, folder: &Path) -> Vec<String> {
        self.running
            .iter()
            .filter(|path| within(path, folder))
            .cloned()
            .collect()
    }

    fn launchd_job(&self, plist: &Path) -> Option<LaunchdJob> {
        self.jobs.get(plist).cloned()
    }

    fn uid(&self) -> Option<u32> {
        Some(501)
    }

    fn reveal(&self, path: &Path) -> io::Result<()> {
        self.revealed.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }

    fn askpass(&self) -> io::Result<PathBuf> {
        Ok(self.dir.path().join("askpass"))
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

/// A path written as on a Mac (`/Library/…`, or `~/…` in the home folder)
/// on this machine.
pub fn place(machine: &Machine, path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => machine.home().unwrap().join(rest),
        None => machine.at(path),
    }
}

/// An empty file at `path` (see [`place`]), its folders made.
pub fn put_file(machine: &Machine, path: &str) -> PathBuf {
    let path = place(machine, path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"").unwrap();
    path
}

/// A plist at `path` (see [`place`]) saying `job`; with `None` its
/// contents are no job launchd would load.
pub fn put_plist(machine: &mut Machine, path: &str, job: Option<LaunchdJob>) -> PathBuf {
    let path = place(machine, path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"plist").unwrap();
    if let Some(job) = job {
        machine.jobs.insert(path.clone(), job);
    }
    path
}

/// What a launchd plist says: it mentions its label, its program, the
/// apps it names and `also`.
pub fn launchd_job(label: &str, program: &str, associated: &[&str], also: &[&str]) -> LaunchdJob {
    let associated: Vec<String> = associated.iter().map(ToString::to_string).collect();
    let mut mentions = vec![label.to_string(), program.to_string()];
    mentions.extend(associated.iter().cloned());
    mentions.extend(also.iter().map(ToString::to_string));
    LaunchdJob {
        label: label.into(),
        program: Some(program.into()),
        associated,
        mentions,
    }
}

/// Leftovers' paths written as on a Mac (see [`place`]), with their jobs.
pub fn shown(machine: &Machine, leftovers: &[Leftover]) -> Vec<(String, Option<Job>)> {
    let home = machine.home().unwrap();
    let root = machine.at("/");
    leftovers
        .iter()
        .map(|l| {
            let (start, rest) = match l.path.strip_prefix(&home) {
                Ok(rest) => ("~/", rest),
                Err(_) => ("/", l.path.strip_prefix(&root).unwrap()),
            };
            let rest = rest.to_string_lossy().replace('\\', "/");
            (format!("{start}{rest}"), l.job.clone())
        })
        .collect()
}
