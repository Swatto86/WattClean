//! An in-memory registry over a sandbox folder, standing in for Windows in
//! tests and in the WebDriver suite's build, so the cleaner, tuner and
//! watcher run without touching the real machine.

mod seed;
mod system;
mod trace;
mod watch;

pub use seed::{FAKE_MARKER, paths};

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::model::{RawValue, RegPath};
use crate::trace::Write;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FakeKey {
    path: RegPath,
    values: Vec<RawValue>,
    /// Owner and permissions as SDDL; `None` is the default an
    /// administrator-owned key has.
    #[serde(default)]
    security: Option<String>,
}

/// The SDDL of a key Windows protects: owned by TrustedInstaller, readable
/// by administrators, changeable by no one else.
pub const PROTECTED: &str = "O:S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464G:S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464D:P(A;CI;KR;;;BA)(A;CI;KA;;;S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464)";
/// What a key has by default, and after the app takes ownership of it.
pub const DEFAULT_SECURITY: &str = "O:BAG:SYD:(A;CI;KA;;;BA)(A;CI;KA;;;SY)";
pub const OWNED: &str = "O:S-1-5-21-1000-1000-1000-1001D:P(A;OICI;KA;;;S-1-5-21-1000-1000-1000-1001)(A;OICI;KA;;;BA)(A;OICI;KA;;;SY)";

impl FakeKey {
    /// Owned by a service (TrustedInstaller): changes are refused.
    fn protected(&self) -> bool {
        self.security
            .as_deref()
            .is_some_and(|sddl| sddl.starts_with("O:S-1-5-80-"))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    keys: BTreeMap<String, FakeKey>,
    env: BTreeMap<String, String>,
    elevated: bool,
    restore_points: Vec<String>,
    /// Keys "opened in Registry Editor", for the tests to check.
    opened: Vec<String>,
    /// Web addresses "opened in the browser".
    #[serde(default)]
    opened_urls: Vec<String>,
    explorer_restarts: u32,
    /// Sign-outs and restarts asked for; the test machine does neither.
    sign_outs: u32,
    pc_restarts: u32,
    hive_releases: u32,
    /// Task Scheduler tasks by name, with their XML.
    tasks: BTreeMap<String, String>,
    /// Notifications shown, as title and text.
    notifications: Vec<(String, String)>,
    /// Bumped by every change, so a watch can tell something changed.
    generation: u64,
    /// What event tracing saw (see `trace`); never saved.
    #[serde(skip)]
    writes: VecDeque<Write>,
}

pub struct FakeSystem {
    root: PathBuf,
    state: Arc<Mutex<State>>,
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
            state: Arc::new(Mutex::new(state)),
            persist: None,
        }
    }

    /// A small machine with healthy and broken entries, and their files
    /// under `root`.
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
                state: Arc::new(Mutex::new(state)),
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
        state.generation += 1;
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

    /// Create the key (and its parents) and set these values in it.
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
        state.generation += 1;
    }

    pub fn restore_points(&self) -> Vec<String> {
        self.lock().restore_points.clone()
    }

    pub fn opened_in_regedit(&self) -> Vec<String> {
        self.lock().opened.clone()
    }

    pub fn opened_urls(&self) -> Vec<String> {
        self.lock().opened_urls.clone()
    }

    pub fn explorer_restarts(&self) -> u32 {
        self.lock().explorer_restarts
    }

    pub fn sign_outs(&self) -> u32 {
        self.lock().sign_outs
    }

    pub fn pc_restarts(&self) -> u32 {
        self.lock().pc_restarts
    }

    pub fn tasks(&self) -> BTreeMap<String, String> {
        self.lock().tasks.clone()
    }

    pub fn notifications(&self) -> Vec<(String, String)> {
        self.lock().notifications.clone()
    }

    pub fn hive_releases(&self) -> u32 {
        self.lock().hive_releases
    }

    /// Give a key (and everything beneath it) Windows' protection.
    pub fn protect(&self, key: &RegPath) {
        let folded = key.folded();
        let prefix = format!("{folded}\\");
        for (name, entry) in &mut self.lock().keys {
            if *name == folded || name.starts_with(&prefix) {
                entry.security = Some(PROTECTED.to_owned());
            }
        }
    }

    pub fn security_of(&self, key: &RegPath) -> Option<String> {
        self.lock()
            .keys
            .get(&key.folded())
            .and_then(|entry| entry.security.clone())
    }

    pub fn has_key(&self, key: &RegPath) -> bool {
        self.lock().keys.contains_key(&key.folded())
    }

    pub fn values_of(&self, key: &RegPath) -> Vec<RawValue> {
        self.lock()
            .keys
            .get(&key.folded())
            .map(|entry| entry.values.clone())
            .unwrap_or_default()
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
                security: None,
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
