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
use crate::system::{Drive, Registry, System, split_hive};

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
}

pub struct FakeSystem {
    root: PathBuf,
    state: Mutex<State>,
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
        })
    }

    pub fn drive_c(&self) -> PathBuf {
        self.root.join("C")
    }

    fn profile(&self) -> PathBuf {
        self.drive_c().join(r"Users\Tester")
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
    fn var(&self, name: &str) -> Vec<String> {
        let c = self.drive_c();
        let profile = self.profile();
        let join = |base: &Path, rel: &str| base.join(rel).to_string_lossy().to_string();
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
            root: r"C:\".to_string(),
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
