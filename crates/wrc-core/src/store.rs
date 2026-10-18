//! Crash-safe JSON files: written to a temporary sibling, flushed, then
//! renamed over the old file, so a failed save leaves the last good copy.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Error, Result};

/// The registry cleaner's folder inside WattClean's data folder:
/// `%LOCALAPPDATA%\WattClean\registry`, or `registry` inside
/// `WATTCLEAN_DATA_DIR` when set (tests and checks).
pub fn data_dir() -> std::result::Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("WATTCLEAN_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir).join("registry"));
    }
    std::env::var_os("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("WattClean").join("registry"))
        .ok_or_else(|| "Windows did not report a Local AppData folder".to_owned())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = Path::new(&temp);
    {
        let mut file = fs::File::create(temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(temp, path)
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text =
        serde_json::to_vec_pretty(value).map_err(|error| Error::Invalid(error.to_string()))?;
    write_atomic(path, &text).map_err(|source| Error::file("save", path, source))
}

/// The file's contents, `None` when it does not exist. A file that exists
/// but cannot be parsed is an error and is never replaced by a default.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(Error::file("read", path, source)),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| Error::Unreadable {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_replaces_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("data.json");
        assert_eq!(read_json::<Vec<u32>>(&path).unwrap(), None);
        write_json(&path, &vec![1, 2]).unwrap();
        write_json(&path, &vec![3]).unwrap();
        assert_eq!(read_json::<Vec<u32>>(&path).unwrap(), Some(vec![3]));
        assert!(!dir.path().join("nested").join("data.json.tmp").exists());
    }

    #[test]
    fn a_damaged_file_is_an_error_not_a_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        fs::write(&path, "{ not json").unwrap();
        let error = read_json::<Vec<u32>>(&path).unwrap_err();
        assert!(matches!(error, Error::Unreadable { .. }), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
    }
}
