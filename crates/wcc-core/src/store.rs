//! JSON files in the data folder, written to a temporary file and renamed so
//! an interrupted save leaves the previous file readable.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// The disk cleaner's folder inside WattClean's data folder:
/// `%LOCALAPPDATA%\WattClean\disk`, or `disk` inside `WATTCLEAN_DATA_DIR`
/// (the WebDriver suite isolates each run this way).
pub fn data_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("WATTCLEAN_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir).join("disk"));
    }
    std::env::var_os("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("WattClean").join("disk"))
        .ok_or_else(|| "Windows did not report a Local AppData folder".to_owned())
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
