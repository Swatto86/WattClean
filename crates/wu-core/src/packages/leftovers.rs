//! What a removed program left in the home folder: its settings, caches and
//! data, found by the names it goes by in the folders programs keep them
//! in. Only exact names are matched, never parts of names; generic folder
//! names and names another installed program goes by are never offered.
//! Nothing is pre-selected, and what is chosen goes to the Trash.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{Manager, Package, PackageSystem, tree_size};

/// A file or folder a removed program left behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    pub path: PathBuf,
    pub size: u64,
    pub folder: bool,
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
    let mut add = |key: String| {
        if key.len() >= 3 && !GENERIC.contains(&key.as_str()) && !keys.contains(&key) {
            keys.push(key);
        }
    };
    let id = package.id.to_lowercase();
    match package.manager {
        Manager::AppImage | Manager::MacApp => {}
        Manager::Flatpak => {
            add(id.clone());
            add(id.rsplit('.').next().unwrap_or(&id).to_string());
        }
        _ => {
            for suffix in SUFFIXES {
                if let Some(base) = id.strip_suffix(suffix) {
                    add(base.to_string());
                }
            }
            add(id.clone());
        }
    }
    let name = package.name.trim().to_lowercase();
    add(name.replace(' ', "-"));
    add(name.replace(' ', ""));
    add(name.clone());
    if let Some(bundle) = &package.bundle_id {
        add(bundle.to_lowercase());
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
    ("Library/LaunchAgents", &["{}.plist"]),
    ("Library/Application Scripts", &["{}"]),
    ("", &[".{}"]),
];

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
    let keys = keys_for(removed);
    let theirs: HashSet<String> = installed
        .iter()
        .filter(|other| other.key() != removed.key())
        .flat_map(keys_for)
        .collect();
    let mine: Vec<String> = keys
        .into_iter()
        .filter(|key| !theirs.contains(key))
        .collect();
    let theirs: Vec<String> = theirs.into_iter().collect();
    if mine.is_empty() {
        return Vec::new();
    }
    let places = if system.is_macos() { MACOS } else { LINUX };
    let mut out = Vec::new();
    for (folder, forms) in places {
        let folder = if folder.is_empty() {
            home.clone()
        } else {
            home.join(folder)
        };
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if !named(&name, forms, &mine) || named(&name, forms, &theirs) {
                continue;
            }
            out.push(leftover(&entry.path()));
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn leftover(path: &Path) -> Leftover {
    let folder = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir());
    Leftover {
        path: path.to_path_buf(),
        size: tree_size(path),
        folder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::testing::Machine;

    #[test]
    fn keys_come_from_the_id_the_name_and_the_bundle() {
        let mut chrome = Package::new(Manager::Apt, "google-chrome-stable", "Google Chrome");
        chrome.app = true;
        let keys = keys_for(&chrome);
        assert!(keys.contains(&"google-chrome".to_string()), "{keys:?}");
        assert!(keys.contains(&"googlechrome".to_string()));
        let flatpak = Package::new(
            Manager::Flatpak,
            "org.gimp.GIMP",
            "GNU Image Manipulation Program",
        );
        assert!(keys_for(&flatpak).contains(&"org.gimp.gimp".to_string()));
        let generic = Package::new(Manager::Apt, "fonts", "Fonts");
        assert!(keys_for(&generic).is_empty());
    }

    #[test]
    fn leftovers_are_found_by_exact_name_and_never_another_programs() {
        let machine = Machine::new(&[]);
        let home = machine.home().unwrap();
        for folder in [
            ".config/vlc",
            ".cache/vlc",
            ".local/share/vlc",
            ".vlc",
            ".config/vlc-extras",
            ".config/mpv",
            ".config/firefox",
            ".mozilla",
            ".var/app/org.videolan.VLC",
        ] {
            std::fs::create_dir_all(home.join(folder)).unwrap();
        }
        std::fs::write(home.join(".config/vlc/vlcrc"), b"settings").unwrap();
        std::fs::create_dir_all(home.join(".local/share/applications")).unwrap();
        std::fs::write(home.join(".local/share/applications/vlc.desktop"), b"").unwrap();

        let vlc = Package::new(Manager::Apt, "vlc", "VLC media player");
        let installed = vec![
            vlc.clone(),
            Package::new(Manager::Snap, "firefox", "firefox"),
        ];
        let found = find_leftovers(&machine, &vlc, &installed);
        let names: Vec<String> = found
            .iter()
            .map(|l| {
                l.path
                    .strip_prefix(&home)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            names,
            [
                ".cache/vlc",
                ".config/vlc",
                ".local/share/applications/vlc.desktop",
                ".local/share/vlc",
                ".vlc"
            ]
        );
        assert!(found.iter().any(|l| l.size == 8 && l.folder));

        // Firefox from APT goes while the snap stays: its folders are the snap's too.
        let deb = Package::new(Manager::Apt, "firefox", "Firefox");
        let mut both = installed.clone();
        both.push(deb.clone());
        assert!(find_leftovers(&machine, &deb, &both).is_empty());
    }

    #[test]
    fn macos_leftovers_follow_the_bundle_identifier() {
        let machine = Machine::macos();
        let home = machine.home().unwrap();
        let library = home.join("Library");
        for folder in [
            "Application Support/Slack",
            "Caches/com.tinyspeck.slackmacgap",
            "Containers/com.tinyspeck.slackmacgap",
            "Group Containers/BQR82RBBHL.com.tinyspeck.slackmacgap",
            "Saved Application State/com.tinyspeck.slackmacgap.savedState",
            "Caches/com.apple.Safari",
        ] {
            std::fs::create_dir_all(library.join(folder)).unwrap();
        }
        std::fs::create_dir_all(library.join("Preferences")).unwrap();
        std::fs::write(
            library.join("Preferences/com.tinyspeck.slackmacgap.plist"),
            b"p",
        )
        .unwrap();
        let mut slack = Package::new(Manager::MacApp, "/Applications/Slack.app", "Slack");
        slack.bundle_id = Some("com.tinyspeck.slackmacgap".into());
        let found = find_leftovers(&machine, &slack, std::slice::from_ref(&slack));
        assert_eq!(found.len(), 6, "{found:#?}");
        assert!(!found.iter().any(|l| l.path.ends_with("com.apple.Safari")));
    }
}
