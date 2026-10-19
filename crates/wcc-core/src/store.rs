//! JSON files in the data folder, written to a temporary file and renamed so
//! an interrupted save leaves the previous file readable.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// The disk cleaner's folder inside WattClean's data folder (`disk`).
pub fn data_dir() -> Result<PathBuf, String> {
    app_root().map(|root| root.join("disk"))
}

/// WattClean's data folder: `%LOCALAPPDATA%\WattClean` on Windows,
/// `~/.local/share/WattClean` (or under `XDG_DATA_HOME`) on Linux and
/// `~/Library/Application Support/WattClean` on macOS.
/// `WATTCLEAN_DATA_DIR` replaces it (the WebDriver suite isolates each run
/// this way).
pub fn app_root() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("WATTCLEAN_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    platform_data_folder(&|name| std::env::var_os(name).map(PathBuf::from))
        .map(|base| base.join("WattClean"))
}

/// Where this platform keeps applications' data, from its environment.
fn platform_data_folder(env: &dyn Fn(&str) -> Option<PathBuf>) -> Result<PathBuf, String> {
    let absolute = |name: &str| env(name).filter(|path| path.is_absolute());
    if cfg!(windows) {
        return absolute("LOCALAPPDATA")
            .ok_or_else(|| "Windows did not report a Local AppData folder".to_owned());
    }
    let home = absolute("HOME").filter(|home| home.parent().is_some());
    if cfg!(target_os = "macos") {
        return home
            .map(|home| home.join("Library").join("Application Support"))
            .ok_or_else(|| "macOS did not report a home folder".to_owned());
    }
    absolute("XDG_DATA_HOME")
        .or_else(|| home.map(|home| home.join(".local").join("share")))
        .ok_or_else(|| "Linux did not report a home folder".to_owned())
}

/// Where custom cleaner files and an imported winapp2.ini live.
pub fn cleaners_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("cleaners")
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path)
}

/// The stored value; `None` when the file is missing or unreadable.
pub fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_loads_through_a_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deeper").join("value.json");
        save(&path, &vec![1, 2, 3]).unwrap();
        assert_eq!(load::<Vec<i32>>(&path), Some(vec![1, 2, 3]));
        assert!(!path.with_extension("json.tmp").exists());
        fs::write(&path, "not json").unwrap();
        assert_eq!(load::<Vec<i32>>(&path), None);
    }
}
