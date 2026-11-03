//! The folders definitions name with `%VARIABLES%` (`vars::UNIX_VARIABLES`,
//! and `vars::MACOS_VARIABLES` on macOS): the XDG base and user folders on
//! Linux, the Library folders on macOS.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wcc_core::paths;
use wcc_core::system::Platform;

use crate::protected::{HOME_FOLDERS, SYSTEM_FOLDERS};

/// Every variable's value on this machine, read once.
#[derive(Debug, Clone, Default)]
pub struct Folders {
    values: BTreeMap<String, String>,
}

impl Folders {
    /// The folders of the account this program runs as.
    pub fn of_this_account() -> Self {
        Self::from_env(Platform::current(), &|name| std::env::var(name).ok())
    }

    /// The folders from an environment (`env`), so tests need no real one.
    /// Without `HOME` only the temporary folder is known.
    ///
    /// The temporary folder is emptied of anything two days old and the
    /// XDG cache folder of anything a week old, so `TMPDIR` and the XDG
    /// folders are not trusted when one of them is, or holds, a folder of
    /// the person's (home, Documents…, `.ssh`, `Library`…) or of the
    /// system's: each such value gives way to its usual folder.
    pub fn from_env(platform: Platform, env: &dyn Fn(&str) -> Option<String>) -> Self {
        let set = |name: &str| {
            env(name)
                .map(|value| value.trim().trim_end_matches('/').to_string())
                .filter(|value| value.starts_with('/'))
        };
        let mut kept: Vec<String> = SYSTEM_FOLDERS
            .iter()
            .filter(|folder| !TEMPORARY_FOLDERS.contains(folder))
            .map(ToString::to_string)
            .collect();
        let trusted =
            |name: &str, kept: &[String]| set(name).filter(|folder| holds_none_of(folder, kept));
        let mut values = BTreeMap::new();
        let Some(home) = set("HOME").filter(|home| home != "/") else {
            let temp = trusted("TMPDIR", &kept).unwrap_or_else(|| default_temp(platform));
            for name in ["TEMP", "TMP", "TMPDIR"] {
                values.insert(name.to_string(), temp.clone());
            }
            return Self { values };
        };
        let under = |rel: &str| format!("{home}/{rel}");
        kept.push(home.clone());
        kept.extend(HOME_FOLDERS.iter().map(|rel| under(rel)));
        // Linux reads where the person's folders are from the configuration
        // folder, which must hold none of them either.
        let mut config = trusted("XDG_CONFIG_HOME", &kept).unwrap_or_else(|| under(".config"));
        let mut people = user_folders(platform, &config, &home);
        if !holds_none_of(
            &config,
            &people.iter().map(|(_, f)| f.clone()).collect::<Vec<_>>(),
        ) {
            config = under(".config");
            people = user_folders(platform, &config, &home);
        }
        kept.extend(people.iter().map(|(_, folder)| folder.clone()));
        let temp = trusted("TMPDIR", &kept).unwrap_or_else(|| default_temp(platform));
        let cache = trusted("XDG_CACHE_HOME", &kept).unwrap_or_else(|| under(".cache"));
        let data = trusted("XDG_DATA_HOME", &kept).unwrap_or_else(|| under(".local/share"));
        let mut put = |name: &str, value: String| {
            values.insert(name.to_string(), value);
        };
        for name in ["TEMP", "TMP", "TMPDIR"] {
            put(name, temp.clone());
        }
        put("HOME", home.clone());
        put(
            "XDG_STATE_HOME",
            trusted("XDG_STATE_HOME", &kept).unwrap_or_else(|| under(".local/state")),
        );
        put("XDG_CONFIG_HOME", config.clone());
        put("XDG_CACHE_HOME", cache.clone());
        put("XDG_DATA_HOME", data.clone());
        put(
            "GOPATH",
            set("GOPATH")
                .map(|p| first_of_list(&p))
                .filter(|folder| holds_none_of(folder, &kept))
                .unwrap_or_else(|| under("go")),
        );
        put(
            "CARGO_HOME",
            trusted("CARGO_HOME", &kept).unwrap_or_else(|| under(".cargo")),
        );
        if platform == Platform::Macos {
            if let Some(cache) = darwin_user_cache().filter(|dir| holds_none_of(dir, &kept)) {
                put("DARWIN_USER_CACHE", cache);
            }
            put("CONFIG", under("Library/Application Support"));
            put("CACHE", under("Library/Caches"));
            put("TRASH", under(".Trash"));
            put(
                "HOMEBREW_CACHE",
                trusted("HOMEBREW_CACHE", &kept)
                    .unwrap_or_else(|| under("Library/Caches/Homebrew")),
            );
        } else {
            put("CONFIG", config);
            put("CACHE", cache.clone());
            put("TRASH", format!("{data}/Trash"));
            put(
                "HOMEBREW_CACHE",
                trusted("HOMEBREW_CACHE", &kept).unwrap_or_else(|| format!("{cache}/Homebrew")),
            );
        }
        for (name, folder) in people {
            put(name, folder);
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

/// System folders a temporary folder may be: the system's own.
const TEMPORARY_FOLDERS: [&str; 3] = ["/tmp", "/var/tmp", "/private/tmp"];

/// Each person's own folder by variable: in the home folder on macOS (with
/// Movies for videos), where Linux's `user-dirs.dirs` in `config` puts it.
fn user_folders(platform: Platform, config: &str, home: &str) -> Vec<(&'static str, String)> {
    let dirs = if platform == Platform::Macos {
        String::new()
    } else {
        std::fs::read_to_string(Path::new(config).join("user-dirs.dirs")).unwrap_or_default()
    };
    USER_FOLDERS
        .iter()
        .map(|(name, folder, key)| {
            let folder = if platform == Platform::Macos && *name == "VIDEOS" {
                "Movies"
            } else {
                folder
            };
            let value = user_dir(&dirs, key, home).unwrap_or_else(|| format!("{home}/{folder}"));
            (*name, value)
        })
        .collect()
}

/// Whether `folder` is none of `kept` and holds none of them (`/` holds
/// them all), compared as the guard compares paths (`paths::key`).
fn holds_none_of(folder: &str, kept: &[String]) -> bool {
    let folder = paths::key(Path::new(folder));
    !kept
        .iter()
        .any(|kept| paths::is_within(&paths::key(Path::new(kept)), &folder))
}

/// The temporary folder when `TMPDIR` is unset or not trusted: on macOS the
/// account's own (`/var/folders/…/T`, which macOS knows even when ssh, cron
/// or launchd set no `TMPDIR`), else `/tmp`.
fn default_temp(platform: Platform) -> String {
    #[cfg(target_os = "macos")]
    if platform == Platform::Macos
        && let Some(dir) = crate::sys::user_temp_dir()
    {
        return dir;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = platform;
    "/tmp".to_string()
}

/// The account's Darwin cache folder (`/var/folders/…/C`): macOS's own,
/// not from the environment.
#[cfg(target_os = "macos")]
fn darwin_user_cache() -> Option<String> {
    crate::sys::user_cache_dir()
}

#[cfg(not(target_os = "macos"))]
fn darwin_user_cache() -> Option<String> {
    None
}

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
#[path = "folders_tests.rs"]
mod tests;
