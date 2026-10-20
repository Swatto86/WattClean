//! The names a package's leftovers may carry: from the package manager's
//! own name for it, the names people see, its bundle identifier; never a
//! generic folder name. Which of them each kind of place is searched by.

use std::collections::HashSet;

use super::{Manager, Package, PackageSystem};

/// Folder names shared by many programs or by the desktop itself.
const GENERIC: &[&str] = &[
    "applications",
    "autostart",
    "bin",
    "cache",
    "common",
    "config",
    "data",
    "dbus-1",
    "dconf",
    "doc",
    "environment.d",
    "fonts",
    "gnupg",
    "gtk-2.0",
    "gtk-3.0",
    "gtk-4.0",
    "icons",
    "keyrings",
    "lib",
    "local",
    "man",
    "menus",
    "mime",
    "pulse",
    "share",
    "ssh",
    "systemd",
    "themes",
    "trash",
    "user-dirs",
    "app",
    "apps",
    "desktop",
    "user",
    "default",
    "tmp",
    "temp",
    "logs",
    "log",
    "help",
];

/// Suffixes packages add to a program's own name.
const SUFFIXES: &[&str] = &[
    "-stable",
    "-beta",
    "-dev",
    "-unstable",
    "-nightly",
    "-bin",
    "-git",
    "-desktop",
    "-browser",
];

/// Adds `key` unless it is shorter than `shortest`, generic or already there.
fn add_key(keys: &mut Vec<String>, key: String, shortest: usize) {
    if key.len() >= shortest && !GENERIC.contains(&key.as_str()) && !keys.contains(&key) {
        keys.push(key);
    }
}

/// The lower-case names from the package manager's own name for a package
/// (`signal` for `signal-desktop`), not from the name people see.
fn id_keys(package: &Package) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    let id = package.id.to_lowercase();
    match package.manager {
        Manager::AppImage | Manager::MacApp => {}
        Manager::Flatpak => {
            add_key(&mut keys, id.clone(), 3);
            add_key(
                &mut keys,
                id.rsplit('.').next().unwrap_or(&id).to_string(),
                3,
            );
        }
        _ => {
            for suffix in SUFFIXES {
                if let Some(base) = id.strip_suffix(suffix) {
                    add_key(&mut keys, base.to_string(), 3);
                }
            }
            // A package manager's own id may be two letters (`gh`); a name
            // is three.
            add_key(&mut keys, id.clone(), 2);
        }
    }
    keys
}

/// The lower-case names a package's leftovers may carry.
pub fn keys_for(package: &Package) -> Vec<String> {
    let mut keys = id_keys(package);
    let mut add = |key: String, shortest: usize| add_key(&mut keys, key, shortest);
    for name in std::iter::once(&package.name).chain(&package.known_as) {
        let name = name.trim().to_lowercase();
        add(name.replace(' ', "-"), 3);
        add(name.replace(' ', ""), 3);
        add(name.clone(), 3);
    }
    if let Some(bundle) = &package.bundle_id {
        add(bundle.to_lowercase(), 3);
    }
    keys
}

/// Where command-line tools keep their files: never offered while a
/// program of that name is still installed.
const TOOL_FOLDERS: &[&str] = &["", ".config", ".local/share", ".local/state", ".cache"];

/// The names a removed package's leftovers are looked for by, in each
/// kind of place.
pub(super) struct Keys {
    /// Its own folders: every name of it that is no other program's.
    pub(super) mine: Vec<String>,
    /// The folders command-line tools share: those no program still goes by.
    tools: Vec<String>,
    /// `~/.name`: of those, the package manager's own name only. A
    /// launcher's name is a word people read ("Files" for Nautilus), and
    /// `~/.files` is someone's dotfiles.
    dotted: Vec<String>,
}

impl Keys {
    pub(super) fn new(
        system: &dyn PackageSystem,
        removed: &Package,
        theirs: &HashSet<String>,
    ) -> Self {
        let mine: Vec<String> = keys_for(removed)
            .into_iter()
            .filter(|key| !theirs.contains(key))
            .collect();
        let tools: Vec<String> = mine
            .iter()
            .filter(|key| system.find_program(key).is_none())
            .cloned()
            .collect();
        let own = id_keys(removed);
        let dotted = tools
            .iter()
            .filter(|key| own.contains(key))
            .cloned()
            .collect();
        Self {
            mine,
            tools,
            dotted,
        }
    }

    pub(super) fn for_folder(&self, folder: &str) -> &[String] {
        if folder.is_empty() {
            &self.dotted
        } else if TOOL_FOLDERS.contains(&folder) {
            &self.tools
        } else {
            &self.mine
        }
    }
}
