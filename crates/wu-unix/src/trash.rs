//! Moving a program's leftovers to the Trash, where they can be put back.
//! Linux follows the freedesktop.org Trash specification (the file under
//! `files`, a `.trashinfo` record under `info`); macOS asks Finder, which
//! remembers where each item came from.

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The home folder's Trash (`$XDG_DATA_HOME/Trash`).
pub fn home_trash(home: Option<&Path>) -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(".local").join("share")))?;
    Some(data.join("Trash"))
}

/// Moves `path` into the Trash folder `bin`, with its record. A file on
/// another drive cannot be moved there and is reported, never deleted.
/// What the Trash is made of, when WattClean makes it, is the account's
/// alone, as the desktop's own Trash is: its names and records tell what
/// was deleted, and from where.
pub fn freedesktop(bin: &Path, path: &Path) -> io::Result<()> {
    let files = bin.join("files");
    let info = bin.join("info");
    let mut folders = fs::DirBuilder::new();
    folders.recursive(true).mode(0o700);
    folders.create(&files)?;
    folders.create(&info)?;
    let original = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to move"))?
        .to_string_lossy()
        .into_owned();
    for attempt in 0..1000 {
        let name = if attempt == 0 {
            original.clone()
        } else {
            format!("{original}.{attempt}")
        };
        let record = info.join(format!("{name}.trashinfo"));
        // The record is made first and exclusively: it reserves the name.
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&record)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let written = write!(
            file,
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            percent_encode(&path.to_string_lossy()),
            timestamp(SystemTime::now())
        );
        let moved = written.and_then(|()| fs::rename(path, files.join(&name)));
        if let Err(error) = moved {
            let _ = fs::remove_file(&record);
            return Err(error);
        }
        return Ok(());
    }
    Err(io::Error::other(
        "the Trash already holds too many items of this name",
    ))
}

/// A path as the Trash records it: bytes outside the unreserved set and `/`
/// as `%XX`.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `YYYY-MM-DDThh:mm:ss`, in UTC (the record asks for local time; the
/// date is only shown to people and never compared).
fn timestamp(time: SystemTime) -> String {
    let seconds = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Finder moves the item to the Trash; it asks for an administrator's
/// password itself when the item is not this account's to move.
pub fn with_finder(path: &Path) -> io::Result<()> {
    let text = path.to_string_lossy();
    let escaped = text.replace('\\', r"\\").replace('"', "\\\"");
    let script = format!("tell application \"Finder\" to delete (POSIX file \"{escaped}\")");
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdin(std::process::Stdio::null())
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Finder could not move {text} to the Trash: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_goes_to_the_trash_with_its_record_and_a_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("Trash");
        let first = dir.path().join("my app");
        fs::create_dir_all(first.join("cache")).unwrap();
        freedesktop(&bin, &first).unwrap();
        assert!(!first.exists());
        assert!(bin.join("files/my app/cache").is_dir());
        let record = fs::read_to_string(bin.join("info/my app.trashinfo")).unwrap();
        assert!(record.starts_with("[Trash Info]\nPath=/"), "{record}");
        assert!(record.contains("my%20app\n"), "{record}");
        // The account's alone, whatever the umask lets through.
        let mode = |path: &Path| {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(path).unwrap().permissions().mode() & 0o777
        };
        for folder in [bin.clone(), bin.join("files"), bin.join("info")] {
            assert_eq!(mode(&folder), 0o700, "{}", folder.display());
        }
        assert_eq!(mode(&bin.join("info/my app.trashinfo")), 0o600);

        fs::write(&first, b"again").unwrap();
        freedesktop(&bin, &first).unwrap();
        assert!(bin.join("files/my app.1").is_file());
    }

    #[test]
    fn dates_are_written_as_the_trash_expects() {
        let time = UNIX_EPOCH + std::time::Duration::from_secs(1_729_339_200 + 3_725);
        assert_eq!(timestamp(time), "2024-10-19T13:02:05");
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00");
    }
}
