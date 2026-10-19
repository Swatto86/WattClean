//! The folders definitions name with `%VARIABLES%` (`vars::UNIX_VARIABLES`):
//! the XDG base and user folders on Linux, the Library folders on macOS.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wcc_core::system::Platform;

/// Every variable's value on this machine, read once.
#[derive(Debug, Clone, Default)]
pub struct Folders {
    values: BTreeMap<String, String>,
}

impl Folders {
    /// The folders of the account this program runs as.
    pub fn of_this_account() -> Self {
        Self::from_env(Platform::current(), &|name| {
            let value = std::env::var(name).ok();
            #[cfg(target_os = "macos")]
            if name == "TMPDIR" && !value.as_deref().is_some_and(|v| v.trim().starts_with('/')) {
                return crate::sys::user_temp_dir();
            }
            value
        })
    }

    /// The folders from an environment (`env`), so tests need no real one.
    /// Without `HOME` only the temporary folder is known.
    pub fn from_env(platform: Platform, env: &dyn Fn(&str) -> Option<String>) -> Self {
        let set = |name: &str| {
            env(name)
                .map(|value| value.trim().trim_end_matches('/').to_string())
                .filter(|value| value.starts_with('/'))
        };
        let mut values = BTreeMap::new();
        let temp = set("TMPDIR").unwrap_or_else(|| "/tmp".to_string());
        for name in ["TEMP", "TMP", "TMPDIR"] {
            values.insert(name.to_string(), temp.clone());
        }
        let Some(home) = set("HOME").filter(|home| home != "/") else {
            return Self { values };
        };
        let under = |rel: &str| format!("{home}/{rel}");
        let mut put = |name: &str, value: String| {
            values.insert(name.to_string(), value);
        };
        put("HOME", home.clone());
        let config = set("XDG_CONFIG_HOME").unwrap_or_else(|| under(".config"));
        let cache = set("XDG_CACHE_HOME").unwrap_or_else(|| under(".cache"));
        let data = set("XDG_DATA_HOME").unwrap_or_else(|| under(".local/share"));
        put(
            "XDG_STATE_HOME",
            set("XDG_STATE_HOME").unwrap_or_else(|| under(".local/state")),
        );
        put("XDG_CONFIG_HOME", config.clone());
        put("XDG_CACHE_HOME", cache.clone());
        put("XDG_DATA_HOME", data.clone());
        put(
            "GOPATH",
            set("GOPATH").map_or_else(|| under("go"), |p| first_of_list(&p)),
        );
        put(
            "CARGO_HOME",
            set("CARGO_HOME").unwrap_or_else(|| under(".cargo")),
        );
        if platform == Platform::Macos {
            put("CONFIG", under("Library/Application Support"));
            put("CACHE", under("Library/Caches"));
            put("TRASH", under(".Trash"));
            put(
                "HOMEBREW_CACHE",
                set("HOMEBREW_CACHE").unwrap_or_else(|| under("Library/Caches/Homebrew")),
            );
            for (name, folder, _) in USER_FOLDERS {
                put(
                    name,
                    under(if name == "VIDEOS" { "Movies" } else { folder }),
                );
            }
        } else {
            put("CONFIG", config.clone());
            put("CACHE", cache.clone());
            put("TRASH", format!("{data}/Trash"));
            put(
                "HOMEBREW_CACHE",
                set("HOMEBREW_CACHE").unwrap_or_else(|| format!("{cache}/Homebrew")),
            );
            let dirs = std::fs::read_to_string(Path::new(&config).join("user-dirs.dirs"))
                .unwrap_or_default();
            for (name, folder, key) in USER_FOLDERS {
                let value = user_dir(&dirs, key, &home).unwrap_or_else(|| under(folder));
                put(name, value);
            }
        }
        Self { values }
    }

    /// The value of a variable, by name in any case.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(&name.to_ascii_uppercase())
            .map(String::as_str)
    }

    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.get(name).map(PathBuf::from)
    }
}

/// Each person's own folders: variable, usual name, and the key that moves
/// it in Linux's `user-dirs.dirs`.
const USER_FOLDERS: [(&str, &str, &str); 6] = [
    ("DESKTOP", "Desktop", "XDG_DESKTOP_DIR"),
    ("DOCUMENTS", "Documents", "XDG_DOCUMENTS_DIR"),
    ("DOWNLOADS", "Downloads", "XDG_DOWNLOAD_DIR"),
    ("MUSIC", "Music", "XDG_MUSIC_DIR"),
    ("PICTURES", "Pictures", "XDG_PICTURES_DIR"),
    ("VIDEOS", "Videos", "XDG_VIDEOS_DIR"),
];

/// The first folder of a `:`-separated list (`GOPATH` may hold several).
fn first_of_list(list: &str) -> String {
    list.split(':').next().unwrap_or(list).to_string()
}

/// A folder from `user-dirs.dirs` (`XDG_DESKTOP_DIR="$HOME/Desktop"`);
/// `None` when it is missing, not quoted, or names the home folder itself.
fn user_dir(text: &str, key: &str, home: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(key) && line[key.len()..].trim_start().starts_with('='))?;
    let value = line.split_once('=')?.1.trim();
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    let value = match value.strip_prefix("$HOME") {
        Some(rest) => format!("{home}{rest}"),
        None => value.to_string(),
    };
    let value = value.trim_end_matches('/').to_string();
    (value.starts_with('/') && value != home).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn linux_folders_follow_xdg_and_fall_back_to_its_defaults() {
        let folders = Folders::from_env(
            Platform::Linux,
            &env(&[("HOME", "/home/t/"), ("XDG_CACHE_HOME", "/var/cache/t")]),
        );
        assert_eq!(folders.get("home"), Some("/home/t"));
        assert_eq!(folders.get("CACHE"), Some("/var/cache/t"));
        assert_eq!(folders.get("XDG_CONFIG_HOME"), Some("/home/t/.config"));
        assert_eq!(folders.get("TRASH"), Some("/home/t/.local/share/Trash"));
        assert_eq!(folders.get("TEMP"), Some("/tmp"));
        assert_eq!(folders.get("HOMEBREW_CACHE"), Some("/var/cache/t/Homebrew"));
        assert_eq!(folders.get("DOWNLOADS"), Some("/home/t/Downloads"));
        for name in wcc_core::vars::UNIX_VARIABLES {
            assert!(folders.get(name).is_some(), "{name} has no value");
        }
    }

    #[test]
    fn macos_folders_are_in_the_library() {
        let folders = Folders::from_env(
            Platform::Macos,
            &env(&[("HOME", "/Users/t"), ("TMPDIR", "/var/folders/ab/xyz/T/")]),
        );
        assert_eq!(
            folders.get("CONFIG"),
            Some("/Users/t/Library/Application Support")
        );
        assert_eq!(folders.get("CACHE"), Some("/Users/t/Library/Caches"));
        assert_eq!(folders.get("TRASH"), Some("/Users/t/.Trash"));
        assert_eq!(folders.get("TEMP"), Some("/var/folders/ab/xyz/T"));
        for name in wcc_core::vars::UNIX_VARIABLES {
            assert!(folders.get(name).is_some(), "{name} has no value");
        }
    }

    #[test]
    fn no_home_means_no_home_folders() {
        let folders = Folders::from_env(Platform::Linux, &env(&[("HOME", "/")]));
        assert_eq!(folders.get("HOME"), None);
        assert_eq!(folders.get("CACHE"), None);
        assert_eq!(folders.get("TEMP"), Some("/tmp"));
    }

    #[test]
    fn user_dirs_are_read_with_home_expanded() {
        let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\nXDG_MUSIC_DIR=\"$HOME/\"\nXDG_VIDEOS_DIR=\"/data/videos\"\n";
        assert_eq!(
            user_dir(text, "XDG_DESKTOP_DIR", "/home/t").as_deref(),
            Some("/home/t/Schreibtisch")
        );
        assert_eq!(user_dir(text, "XDG_MUSIC_DIR", "/home/t"), None);
        assert_eq!(
            user_dir(text, "XDG_VIDEOS_DIR", "/home/t").as_deref(),
            Some("/data/videos")
        );
        assert_eq!(user_dir(text, "XDG_DOWNLOAD_DIR", "/home/t"), None);
    }
}
