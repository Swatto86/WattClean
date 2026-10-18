//! An in-memory registry over a sandbox folder, standing in for Windows in
//! tests and in the WebDriver suite's build. Its "uninstallers" apply a
//! scripted effect, so a whole uninstall runs without touching the machine.

mod install;
mod seed;
mod system;

pub use install::InstallEffect;

pub use seed::accounts::{BUNDLE_ID, BUNDLE_PACKAGE};
pub use seed::tailspin::SETUP as TAILSPIN_SETUP;
pub use seed::windows::{APP_ID, INTERFACE_ID, TASK_ID};
pub use seed::{VIEWER_CODE, VIEWER_PACKED};

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::model::{RawValue, RegPath};
use crate::system::{Launch, Running};

/// What one fake uninstaller does when started: matched by a fragment of
/// its command line (lower case).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effect {
    pub command_contains: String,
    pub delete_keys: Vec<RegPath>,
    pub delete_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FakeKey {
    path: RegPath,
    values: Vec<RawValue>,
    modified: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    keys: BTreeMap<String, FakeKey>,
    env: BTreeMap<String, String>,
    elevated: bool,
    effects: Vec<Effect>,
    #[serde(default)]
    installs: Vec<InstallEffect>,
    launches: Vec<Launch>,
    restore_points: Vec<String>,
    reboot_deletes: Vec<PathBuf>,
    #[serde(default)]
    hive_releases: u32,
}

pub struct FakeSystem {
    root: PathBuf,
    state: Mutex<State>,
    /// Where the registry is saved after each change, so a restarted app
    /// sees the machine as the last run left it.
    persist: Option<PathBuf>,
}

impl FakeSystem {
    /// No keys, no variables, elevated.
    pub fn empty(root: &Path) -> Self {
        let state = State {
            elevated: true,
            ..State::default()
        };
        Self {
            root: root.to_path_buf(),
            state: Mutex::new(state),
            persist: None,
        }
    }

    /// A small machine with a handful of programs and their files under `root`.
    pub fn seeded(root: &Path) -> io::Result<Self> {
        let system = Self::empty(root);
        seed::seed(&system)?;
        Ok(system)
    }

    /// The seeded machine, saved as `registry.json` under `root` and reloaded
    /// from there when it exists.
    pub fn persistent(root: &Path) -> io::Result<Self> {
        let file = root.join("registry.json");
        if file.is_file() {
            let text = std::fs::read_to_string(&file)?;
            let state: State = serde_json::from_str(&text).map_err(io::Error::other)?;
            return Ok(Self {
                root: root.to_path_buf(),
                state: Mutex::new(state),
                persist: Some(file),
            });
        }
        std::fs::create_dir_all(root)?;
        let mut system = Self::seeded(root)?;
        system.persist = Some(file);
        system.save(&system.lock())?;
        Ok(system)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A poisoned lock only means another test thread panicked; the map is
        // still consistent because every change is a single insert or remove.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn save(&self, state: &State) -> io::Result<()> {
        if let Some(file) = &self.persist {
            let text = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
            crate::store::write_atomic(file, text.as_bytes())?;
        }
        Ok(())
    }

    fn mutate<T>(&self, change: impl FnOnce(&mut State) -> io::Result<T>) -> io::Result<T> {
        let mut state = self.lock();
        let result = change(&mut state)?;
        self.save(&state)?;
        Ok(result)
    }

    pub fn set_env(&self, name: &str, value: &str) {
        self.lock()
            .env
            .insert(name.to_uppercase(), value.to_owned());
    }

    pub fn set_elevated(&self, elevated: bool) {
        self.lock().elevated = elevated;
    }

    pub fn add_effect(&self, effect: Effect) {
        self.lock().effects.push(effect);
    }

    pub fn add_install(&self, effect: InstallEffect) {
        self.lock().installs.push(effect);
    }

    pub fn put(&self, key: &RegPath, values: Vec<RawValue>) {
        let mut state = self.lock();
        ensure_key(&mut state, key);
        if let Some(entry) = state.keys.get_mut(&key.folded()) {
            for value in values {
                entry
                    .values
                    .retain(|old| !old.name.eq_ignore_ascii_case(&value.name));
                entry.values.push(value);
            }
        }
    }

    pub fn set_modified(&self, key: &RegPath, seconds: u64) {
        if let Some(entry) = self.lock().keys.get_mut(&key.folded()) {
            entry.modified = seconds;
        }
    }

    pub fn launches(&self) -> Vec<Launch> {
        self.lock().launches.clone()
    }

    pub fn restore_points(&self) -> Vec<String> {
        self.lock().restore_points.clone()
    }

    /// How often account hives were released.
    pub fn hive_releases(&self) -> u32 {
        self.lock().hive_releases
    }

    pub fn reboot_deletes(&self) -> Vec<PathBuf> {
        self.lock().reboot_deletes.clone()
    }

    pub fn values_of(&self, key: &RegPath) -> Vec<RawValue> {
        self.lock()
            .keys
            .get(&key.folded())
            .map(|entry| entry.values.clone())
            .unwrap_or_default()
    }

    pub fn has_key(&self, key: &RegPath) -> bool {
        self.lock().keys.contains_key(&key.folded())
    }
}

fn ensure_key(state: &mut State, key: &RegPath) {
    let mut current = RegPath::new(key.hive, "");
    for part in key.path.split('\\').filter(|part| !part.is_empty()) {
        current = current.child(part);
        state
            .keys
            .entry(current.folded())
            .or_insert_with(|| FakeKey {
                path: current.clone(),
                values: Vec::new(),
                modified: 1_772_582_400,
            });
    }
}

fn not_found(key: &RegPath) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{} does not exist", key.display()),
    )
}

fn children<'a>(state: &'a State, key: &RegPath) -> impl Iterator<Item = &'a FakeKey> + 'a {
    let prefix = if key.path.is_empty() {
        format!("{}\\", key.hive.short())
    } else {
        format!("{}\\", key.folded())
    };
    let depth = key.depth() + 1;
    state
        .keys
        .range(prefix.clone()..)
        .take_while(move |(folded, _)| folded.starts_with(&prefix))
        .filter(move |(_, entry)| entry.path.depth() == depth)
        .map(|(_, entry)| entry)
}

/// Reports finished after a couple of polls, like a short uninstaller.
struct FakeRunning {
    polls_left: u32,
}

impl Running for FakeRunning {
    fn finished(&mut self) -> io::Result<bool> {
        self.polls_left = self.polls_left.saturating_sub(1);
        Ok(self.polls_left == 0)
    }
}
