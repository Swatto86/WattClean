//! A test machine inside a sandbox folder, for the WebDriver suite and for
//! development without touching the real PC. Files are real files under
//! `<sandbox>\machine\C`; the registry, clipboard, running programs and
//! restart deletions live in `<sandbox>\machine\state.json`, saved after
//! every change so a test can read what the app did.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::store;
use crate::system::{Drive, Platform, ProgramOutput, Registry, System, split_hive};

/// Text only a build with the test machine contains; the suite checks for it.
pub const MARKER: &str = "Northwind Browser (test machine)";
/// The wipe on the test machine stops after this many bytes.
pub const WIPE_LIMIT: u64 = 3 << 20;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Keys (hive in short form, lower case) and the value names in each.
    pub registry: BTreeMap<String, Vec<String>>,
    pub clipboard: bool,
    pub running: Vec<String>,
    pub reboot_deletions: Vec<String>,
    pub shell_notifications: u32,
    /// Programs a task may run, with what each prints whenever it runs.
    pub programs: BTreeMap<String, String>,
    /// Programs only in a folder the account can write (as Homebrew's is):
    /// found for a task that runs as the account, never for one that runs
    /// as administrator.
    pub untrusted: Vec<String>,
    /// Every program run, as `name arg …`, after any `VARIABLE=value` it
    /// was given and `admin ` first when it asked for administrator rights.
    pub ran: Vec<String>,
}

pub struct FakeSystem {
    root: PathBuf,
    state: Mutex<State>,
    /// The system the engine is told it runs on: Windows unless a test
    /// says otherwise (`posing_as`).
    platform: Platform,
}

/// A key as the test machine files it: short hive, the rest lower case.
pub fn fold(key: &str) -> Option<String> {
    let (hive, rest) = split_hive(key)?;
    Some(if rest.is_empty() {
        hive.to_string()
    } else {
        format!(r"{hive}\{}", rest.to_lowercase())
    })
}

impl FakeSystem {
    /// Opens (or seeds, on first use) the test machine in `sandbox`.
    pub fn open(sandbox: &Path) -> io::Result<Self> {
        let root = sandbox.join("machine");
        if !root.join("state.json").exists() {
            crate::fake_seed::seed(sandbox)?;
        }
        let state = store::load(&root.join("state.json")).unwrap_or_default();
        Ok(Self {
            root,
            state: Mutex::new(state),
            platform: Platform::Windows,
        })
    }

    /// Tells the engine this machine runs `platform`, for tests of what it
    /// does differently there; the files stay shaped as a Windows PC's.
    #[must_use]
    pub fn posing_as(mut self, platform: Platform) -> Self {
        self.platform = platform;
        self
    }

    pub fn drive_c(&self) -> PathBuf {
        self.root.join("C")
    }

    fn profile(&self) -> PathBuf {
        crate::paths::join(&self.drive_c(), r"Users\Tester")
    }

    /// Gives the test machine a program for tasks to find and run.
    pub fn add_program(&self, name: &str, prints: &str) -> io::Result<()> {
        self.with_state(|s| {
            s.programs.insert(name.to_string(), prints.to_string());
        })
    }

    /// Gives the test machine a program only in a folder the account can
    /// write, where any program the account runs could have put it.
    pub fn add_untrusted_program(&self, name: &str, prints: &str) -> io::Result<()> {
        self.with_state(|s| {
            s.programs.insert(name.to_string(), prints.to_string());
            s.untrusted.push(name.to_string());
        })
    }

    fn untrusted(&self, name: &str) -> bool {
        self.read(|s| s.untrusted.iter().any(|n| n == name))
            .unwrap_or(false)
    }

    /// Every program run so far (see [`State::ran`]).
    pub fn ran(&self) -> Vec<String> {
        self.read(|s| s.ran.clone()).unwrap_or_default()
    }

    fn with_state<T>(&self, change: impl FnOnce(&mut State) -> T) -> io::Result<T> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("test machine state poisoned"))?;
        let result = change(&mut state);
        store::save(&self.root.join("state.json"), &*state)?;
        Ok(result)
    }

    fn read<T>(&self, look: impl FnOnce(&State) -> T) -> Option<T> {
        self.state.lock().ok().map(|state| look(&state))
    }

    fn recycle_dir(&self) -> PathBuf {
        self.drive_c().join("$Recycle.Bin")
    }
}

fn tree_size(dir: &Path) -> (u64, u64) {
    let mut items = 0;
    let mut bytes = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                let (i, b) = tree_size(&entry.path());
                items += i;
                bytes += b;
            } else {
                items += 1;
                bytes += meta.len();
            }
        }
    }
    (items, bytes)
}

impl System for FakeSystem {
    /// The test machine is shaped as a Windows PC on every platform, and
    /// says Windows unless a test made it pose as another.
    fn platform(&self) -> Platform {
        self.platform
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        self.read(|s| s.programs.contains_key(name))
            .unwrap_or(false)
            .then(|| self.root.join("bin").join(name))
    }

    fn find_admin_program(&self, name: &str) -> Option<PathBuf> {
        self.find_program(name).filter(|_| !self.untrusted(name))
    }

    fn run_program(
        &self,
        program: &Path,
        args: &[String],
        admin: bool,
        env: &[(&str, &str)],
    ) -> io::Result<ProgramOutput> {
        let name = program
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let stdout = self
            .read(|s| s.programs.get(&name).cloned())
            .flatten()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        if admin && self.untrusted(&name) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "not run with administrator rights",
            ));
        }
        let mut line = if admin {
            "admin ".to_string()
        } else {
            String::new()
        };
        for (variable, value) in env {
            line.push_str(&format!("{variable}={value} "));
        }
        line.push_str(&name);
        for arg in args {
            line.push(' ');
            line.push_str(arg);
        }
        self.with_state(|s| s.ran.push(line))?;
        Ok(ProgramOutput {
            success: true,
            stdout,
            stderr: String::new(),
        })
    }

    fn var(&self, name: &str) -> Vec<String> {
        let c = self.drive_c();
        let profile = self.profile();
        let join =
            |base: &Path, rel: &str| crate::paths::join(base, rel).to_string_lossy().to_string();
        let one = |p: String| vec![p];
        match name.to_ascii_uppercase().as_str() {
            "USERPROFILE" => one(profile.to_string_lossy().to_string()),
            "APPDATA" => one(join(&profile, r"AppData\Roaming")),
            "LOCALAPPDATA" => one(join(&profile, r"AppData\Local")),
            "LOCALLOWAPPDATA" => one(join(&profile, r"AppData\LocalLow")),
            "TEMP" | "TMP" => one(join(&profile, r"AppData\Local\Temp")),
            "WINDIR" | "SYSTEMROOT" => one(join(&c, "Windows")),
            "SYSTEMDRIVE" => one(c.to_string_lossy().to_string()),
            "PROGRAMDATA" | "COMMONAPPDATA" | "ALLUSERSPROFILE" => one(join(&c, "ProgramData")),
            "PROGRAMFILES" => vec![join(&c, "Program Files"), join(&c, "Program Files (x86)")],
            "PROGRAMFILES(X86)" => one(join(&c, "Program Files (x86)")),
            "PROGRAMW6432" => one(join(&c, "Program Files")),
            "COMMONPROGRAMFILES" => vec![
                join(&c, r"Program Files\Common Files"),
                join(&c, r"Program Files (x86)\Common Files"),
            ],
            "PUBLIC" => one(join(&c, r"Users\Public")),
            "DOCUMENTS" => one(join(&profile, "Documents")),
            "DESKTOP" => one(join(&profile, "Desktop")),
            "DOWNLOADS" => one(join(&profile, "Downloads")),
            "PICTURES" => one(join(&profile, "Pictures")),
            "MUSIC" => one(join(&profile, "Music")),
            "VIDEO" => one(join(&profile, "Videos")),
            "FAVORITES" => one(join(&profile, "Favorites")),
            "STARTMENU" => one(join(
                &profile,
                r"AppData\Roaming\Microsoft\Windows\Start Menu",
            )),
            "ONEDRIVE" => one(join(&profile, "OneDrive")),
            _ => Vec::new(),
        }
    }

    fn registry(&self) -> &dyn Registry {
        self
    }

    fn recycle_bin(&self) -> io::Result<(u64, u64)> {
        Ok(tree_size(&self.recycle_dir()))
    }

    fn empty_recycle_bin(&self) -> io::Result<()> {
        let dir = self.recycle_dir();
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)
    }

    fn clipboard_has_data(&self) -> bool {
        self.read(|s| s.clipboard).unwrap_or(false)
    }

    fn clear_clipboard(&self) -> io::Result<()> {
        self.with_state(|s| s.clipboard = false)
    }

    fn running(&self) -> HashSet<String> {
        self.read(|s| s.running.iter().map(|r| r.to_ascii_lowercase()).collect())
            .unwrap_or_default()
    }

    fn is_elevated(&self) -> bool {
        false
    }

    fn drives(&self) -> Vec<Drive> {
        vec![Drive {
            // The sandbox lives on the host's own drive: on Linux and macOS
            // that is the root, which holds the sandbox's temporary folder.
            root: if cfg!(windows) { r"C:\" } else { "/" }.to_string(),
            label: "Test machine".to_string(),
            file_system: "NTFS".to_string(),
            free: WIPE_LIMIT,
            total: 64 << 20,
            removable: false,
        }]
    }

    fn free_space(&self, _path: &Path) -> io::Result<u64> {
        Ok(WIPE_LIMIT)
    }

    fn delete_on_reboot(&self, path: &Path) -> io::Result<()> {
        self.with_state(|s| s.reboot_deletions.push(path.to_string_lossy().to_string()))
    }

    /// The test machine's files have one name each: hard links cannot be
    /// counted without Win32 calls (the real count is tested in
    /// `wcc-windows`).
    fn link_count(&self, path: &Path) -> io::Result<u32> {
        std::fs::symlink_metadata(path).map(|_| 1)
    }

    fn shell_changed(&self) {
        let _saved = self.with_state(|s| s.shell_notifications += 1);
    }

    fn wipe_limit(&self) -> Option<u64> {
        Some(WIPE_LIMIT)
    }

    fn wipe_folder(&self, _drive: &Drive) -> PathBuf {
        self.drive_c()
    }
}

impl Registry for FakeSystem {
    fn key_exists(&self, key: &str) -> bool {
        let Some(key) = fold(key) else { return false };
        self.read(|s| s.registry.keys().any(|k| crate::paths::is_within(k, &key)))
            .unwrap_or(false)
    }

    fn value_exists(&self, key: &str, name: &str) -> bool {
        let Some(key) = fold(key) else { return false };
        self.read(|s| {
            s.registry
                .get(&key)
                .is_some_and(|values| values.iter().any(|v| v.eq_ignore_ascii_case(name)))
        })
        .unwrap_or(false)
    }

    fn subkeys(&self, key: &str) -> Vec<String> {
        let Some(key) = fold(key) else {
            return Vec::new();
        };
        let prefix = format!(r"{key}\");
        self.read(|s| {
            let mut names: Vec<String> = s
                .registry
                .keys()
                .filter_map(|k| k.strip_prefix(&prefix))
                .filter_map(|rest| rest.split('\\').next())
                .map(str::to_string)
                .collect();
            names.sort();
            names.dedup();
            names
        })
        .unwrap_or_default()
    }

    fn delete_key(&self, key: &str) -> io::Result<()> {
        let key = fold(key).ok_or_else(|| io::Error::other("unknown hive"))?;
        self.with_state(|s| s.registry.retain(|k, _| !crate::paths::is_within(k, &key)))
    }

    fn delete_value(&self, key: &str, name: &str) -> io::Result<()> {
        let key = fold(key).ok_or_else(|| io::Error::other("unknown hive"))?;
        self.with_state(|s| {
            if let Some(values) = s.registry.get_mut(&key) {
                values.retain(|v| !v.eq_ignore_ascii_case(name));
            }
        })
    }
}
