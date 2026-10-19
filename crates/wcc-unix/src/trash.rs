//! The Trash. On Linux it is the freedesktop.org Trash: the home folder's
//! (`~/.local/share/Trash`) and one on each other drive (`.Trash-<uid>` or
//! `.Trash/<uid>`), each a `files` folder with an `info` file per item. On
//! macOS Finder empties it: the Trash folder is protected by macOS, and
//! Finder knows every drive's.

#[cfg(not(target_os = "macos"))]
use std::fs;
use std::io;
#[cfg(not(target_os = "macos"))]
use std::path::{Path, PathBuf};

#[cfg(not(target_os = "macos"))]
/// The Trash folders of this account, those that exist.
pub fn linux_folders(home_trash: Option<&Path>, drive_roots: &[String], uid: u32) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = home_trash.map(Path::to_path_buf).into_iter().collect();
    for root in drive_roots {
        let root = Path::new(root);
        folders.push(root.join(format!(".Trash-{uid}")));
        folders.push(root.join(".Trash").join(uid.to_string()));
    }
    folders.retain(|folder| folder.join("files").is_dir());
    folders.dedup();
    folders
}

#[cfg(not(target_os = "macos"))]
/// Items (the top-level entries of `files`) and their bytes.
pub fn count(folders: &[PathBuf]) -> io::Result<(u64, u64)> {
    let mut items = 0;
    let mut bytes = 0;
    for folder in folders {
        for entry in fs::read_dir(folder.join("files"))? {
            let entry = entry?;
            items += 1;
            bytes += tree_size(&entry.path());
        }
    }
    Ok((items, bytes))
}

#[cfg(not(target_os = "macos"))]
fn tree_size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(path)
        .map(|entries| entries.flatten().map(|e| tree_size(&e.path())).sum())
        .unwrap_or(0)
}

#[cfg(not(target_os = "macos"))]
/// Removes every item and its record; links are removed, never followed.
/// The first failure is reported after trying everything else.
pub fn empty(folders: &[PathBuf]) -> io::Result<()> {
    let mut first_error = None;
    for folder in folders {
        for part in ["files", "info", "expunged"] {
            let Ok(entries) = fs::read_dir(folder.join(part)) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let removed = match entry.file_type() {
                    Ok(kind) if kind.is_dir() => fs::remove_dir_all(&path),
                    _ => fs::remove_file(&path),
                };
                if let Err(error) = removed {
                    first_error.get_or_insert(io::Error::new(
                        error.kind(),
                        format!("{}: {error}", path.display()),
                    ));
                }
            }
        }
        let sizes = folder.join("directorysizes");
        if sizes.exists()
            && let Err(error) = fs::remove_file(&sizes)
        {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Asks Finder, which may need the person's consent the first time.
#[cfg(target_os = "macos")]
pub fn finder(script: &str) -> io::Result<String> {
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdin(std::process::Stdio::null())
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(io::Error::other(format!(
            "Finder could not do it: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(test)]
#[cfg(not(target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn a_trash_is_counted_and_emptied_links_and_all() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("Trash");
        let outside = dir.path().join("kept.txt");
        fs::write(&outside, b"keep").unwrap();
        fs::create_dir_all(trash.join("files/folder/sub")).unwrap();
        fs::create_dir_all(trash.join("info")).unwrap();
        fs::write(trash.join("files/folder/sub/a.bin"), b"12345").unwrap();
        fs::write(trash.join("files/b.txt"), b"123").unwrap();
        std::os::unix::fs::symlink(&outside, trash.join("files/link")).unwrap();
        fs::write(trash.join("info/b.txt.trashinfo"), b"[Trash Info]").unwrap();
        fs::write(trash.join("directorysizes"), b"").unwrap();

        let folders = linux_folders(Some(&trash), &[], 1000);
        assert_eq!(folders, vec![trash.clone()]);
        let (items, bytes) = count(&folders).unwrap();
        assert_eq!(items, 3);
        assert!(bytes >= 8);

        empty(&folders).unwrap();
        assert_eq!(count(&folders).unwrap().0, 0);
        assert_eq!(fs::read_dir(trash.join("info")).unwrap().count(), 0);
        assert!(!trash.join("directorysizes").exists());
        assert!(outside.exists(), "a link's target is never touched");
    }

    #[test]
    fn other_drives_trash_folders_are_found_by_user_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        fs::create_dir_all(dir.path().join(".Trash-1000/files")).unwrap();
        fs::create_dir_all(dir.path().join(".Trash/1000/files")).unwrap();
        fs::create_dir_all(dir.path().join(".Trash-1001/files")).unwrap();
        let folders = linux_folders(None, &[root], 1000);
        assert_eq!(folders.len(), 2);
    }
}
