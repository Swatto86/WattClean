//! What a removed program left in the home folder: its settings, caches and
//! data, found by the names it goes by in the folders programs keep them
//! in. Names are matched whole, never as parts, except that on macOS an
//! entry named after the app's bundle identifier plus a suffix
//! (`com.vendor.App.ShipIt`) is the app's unless a longer installed
//! identifier owns it, and a team or group prefix (`<TeamID>.<id>`,
//! `group.<id>`) is taken before the bundle identifier only. Generic folder
//! names, names another installed program goes by, Apple's own entries
//! (unless the app is Apple's) and the folders of a command-line tool still
//! installed (Apple's git, Claude Code's `~/.claude`) are never offered.
//! Nothing is pre-selected, and what is chosen goes to the Trash. `extra`
//! adds a cask's own zap list and the folder a lone app came in.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{Manager, Package, PackageSystem, extra, launchd, tree_size};

/// A file or folder a removed program left behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    pub path: PathBuf,
    /// `None` when macOS will not let WattClean look inside.
    pub size: Option<u64>,
    pub folder: bool,
    /// The label of the background job this plist starts in the account's
    /// session: stopped before the plist goes to the Trash.
    pub job: Option<String>,
}

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

/// The lower-case names a package's leftovers may carry.
pub fn keys_for(package: &Package) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    // A package manager's own id may be two letters (`gh`); a name is three.
    let mut add = |key: String, shortest: usize| {
        if key.len() >= shortest && !GENERIC.contains(&key.as_str()) && !keys.contains(&key) {
            keys.push(key);
        }
    };
    let id = package.id.to_lowercase();
    match package.manager {
        Manager::AppImage | Manager::MacApp => {}
        Manager::Flatpak => {
            add(id.clone(), 3);
            add(id.rsplit('.').next().unwrap_or(&id).to_string(), 3);
        }
        _ => {
            for suffix in SUFFIXES {
                if let Some(base) = id.strip_suffix(suffix) {
                    add(base.to_string(), 3);
                }
            }
            add(id.clone(), 2);
        }
    }
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

/// Where programs keep their own files, under the home folder, and how an
/// entry there is named after a key: `{}` is the key.
const LINUX: &[(&str, &[&str])] = &[
    (".config", &["{}"]),
    (".local/share", &["{}"]),
    (".local/state", &["{}"]),
    (".cache", &["{}"]),
    ("", &[".{}"]),
    (".var/app", &["{}"]),
    ("snap", &["{}"]),
    (".local/share/applications", &["{}.desktop"]),
    (".config/autostart", &["{}.desktop"]),
];

const MACOS: &[(&str, &[&str])] = &[
    (".config", &["{}"]),
    (".local/share", &["{}"]),
    (".local/state", &["{}"]),
    (".cache", &["{}"]),
    ("Library/Application Support", &["{}"]),
    ("Library/Caches", &["{}"]),
    ("Library/Preferences", &["{}.plist"]),
    ("Library/Containers", &["{}"]),
    ("Library/Group Containers", &["*.{}", "group.{}"]),
    ("Library/Saved Application State", &["{}.savedstate"]),
    ("Library/Logs", &["{}"]),
    ("Library/HTTPStorages", &["{}", "{}.binarycookies"]),
    ("Library/WebKit", &["{}"]),
    ("Library/Cookies", &["{}.binarycookies"]),
    ("Library/Application Scripts", &["{}", "*.{}", "group.{}"]),
    ("", &[".{}"]),
];

/// Where command-line tools keep their files: never offered while a
/// program of that name is still installed.
const TOOL_FOLDERS: &[&str] = &["", ".config", ".local/share", ".local/state", ".cache"];

/// Where macOS apps keep entries named after their bundle identifier plus a
/// suffix (`.ShipIt` updates, `.FinderSyncExt` extensions).
const SUFFIXED: &[&str] = &[
    "Library/Caches",
    "Library/Containers",
    "Library/Application Scripts",
];

/// Forms that put a team or group prefix before a bundle identifier
/// (`<TeamID>.<id>`, `group.<id>`): taken with the removed app's bundle
/// identifier only, since a plain name such as `tag` ends many of Apple's
/// own entries (`com.apple.news.tag`).
fn prefixed(form: &&str) -> bool {
    form.starts_with("*.") || form.starts_with("group.")
}

/// Whether an entry's name (lower case) is Apple's: `com.apple.…`,
/// `group.com.apple.…` or `<TeamID>.com.apple.…`.
fn apples(name: &str) -> bool {
    name.starts_with("com.apple.") || name.contains(".com.apple.")
}

/// Whether an entry's name (lower case) is a key in one of the forms.
fn named(entry: &str, forms: &[&str], keys: &[String]) -> bool {
    keys.iter().any(|key| {
        forms.iter().any(|form| match form.strip_prefix("*.") {
            Some(rest) => entry.ends_with(&format!(".{}", rest.replace("{}", key))),
            None => entry == form.replace("{}", key),
        })
    })
}

/// The leftovers of `removed`, given everything still installed.
pub fn find_leftovers(
    system: &dyn PackageSystem,
    removed: &Package,
    installed: &[Package],
) -> Vec<Leftover> {
    let Some(home) = system.home() else {
        return Vec::new();
    };
    let others: Vec<&Package> = installed
        .iter()
        .filter(|other| other.key() != removed.key())
        .collect();
    // Another copy of the same app (one bundle identifier) shares every
    // file the app keeps, whatever names the copies go by.
    if let Some(id) = &removed.bundle_id
        && others.iter().any(|other| {
            other
                .bundle_id
                .as_ref()
                .is_some_and(|o| o.eq_ignore_ascii_case(id))
        })
    {
        return Vec::new();
    }
    let theirs: HashSet<String> = others.iter().flat_map(|other| keys_for(other)).collect();
    let mine: Vec<String> = keys_for(removed)
        .into_iter()
        .filter(|key| !theirs.contains(key))
        .collect();
    let theirs: Vec<String> = theirs.into_iter().collect();
    let tools: Vec<String> = mine
        .iter()
        .filter(|key| system.find_program(key).is_none())
        .cloned()
        .collect();
    // `~/.name` is a command-line tool's: an app's name or a cask's token is
    // shared with other tools, and apps keep their files elsewhere.
    let gui = matches!(
        removed.manager,
        Manager::MacApp
            | Manager::HomebrewCask
            | Manager::Flatpak
            | Manager::Snap
            | Manager::AppImage
    );
    let bundle = removed
        .bundle_id
        .as_deref()
        .map(str::to_lowercase)
        .filter(|id| id.matches('.').count() >= 2);
    let other_bundles: Vec<String> = others
        .iter()
        .filter_map(|other| other.bundle_id.as_deref())
        .map(str::to_lowercase)
        .collect();
    let places = if system.is_macos() { MACOS } else { LINUX };
    let mut out = extra::leftovers(system, &home, removed, &others, &theirs, &other_bundles);
    for (folder, forms) in places {
        if folder.is_empty() && gui {
            continue;
        }
        let keys = if TOOL_FOLDERS.contains(folder) {
            &tools
        } else {
            &mine
        };
        let suffixed = bundle.as_deref().filter(|_| SUFFIXED.contains(folder));
        if keys.is_empty() && suffixed.is_none() {
            continue;
        }
        let (prefixed_forms, plain_forms): (Vec<&str>, Vec<&str>) =
            forms.iter().copied().partition(prefixed);
        let own_id: &[String] = match &bundle {
            Some(id) if keys.contains(id) => std::slice::from_ref(id),
            _ => &[],
        };
        let dir = if folder.is_empty() {
            home.clone()
        } else {
            home.join(folder)
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let by_name = (named(&name, &plain_forms, keys)
                || named(&name, &prefixed_forms, own_id))
                && !named(&name, forms, &theirs);
            let by_bundle = suffixed.is_some_and(|id| owns(id, &name, &other_bundles));
            if by_name || by_bundle {
                out.push(leftover(&entry.path()));
            }
        }
    }
    out.extend(launchd::leftovers(
        system,
        &home,
        removed,
        &mine,
        &theirs,
        bundle.as_deref(),
        &other_bundles,
    ));
    let mut out = tidy(out, bundle.as_deref());
    launchd::label_agents(system, &home, &mut out);
    out
}

/// Leaves out Apple's own entries unless the app (`bundle`) is Apple's,
/// sorts, and keeps each path once.
fn tidy(mut out: Vec<Leftover>, bundle: Option<&str>) -> Vec<Leftover> {
    // Apple's entries are its own apps' (Notes' group, News' scripts), and
    // Apple's apps are never listed, so no name check can hold them back.
    let apple = bundle.is_some_and(|id| id.starts_with("com.apple."));
    out.retain(|leftover| {
        apple
            || !leftover
                .path
                .file_name()
                .is_some_and(|name| apples(&name.to_string_lossy().to_lowercase()))
    });
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    out
}

/// `entry` is `<id>.<something>`, and no other installed app has a longer
/// identifier that it belongs to (Chrome Canary's `com.google.chrome.canary`
/// when Chrome is removed).
pub(super) fn owns(id: &str, entry: &str, other_ids: &[String]) -> bool {
    entry.len() > id.len() + 1
        && entry.starts_with(id)
        && entry.as_bytes().get(id.len()) == Some(&b'.')
        && !other_ids.iter().any(|other| {
            other.len() > id.len() && (entry == other || entry.starts_with(&format!("{other}.")))
        })
}

pub(super) fn leftover(path: &Path) -> Leftover {
    let folder = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir());
    let readable = !folder || std::fs::read_dir(path).is_ok();
    Leftover {
        path: path.to_path_buf(),
        size: readable.then(|| tree_size(path)),
        folder,
        job: None,
    }
}

#[cfg(test)]
#[path = "leftovers_tests.rs"]
mod tests;
