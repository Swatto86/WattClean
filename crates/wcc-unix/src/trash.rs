//! The Trash. On Linux it is the freedesktop.org Trash: the home folder's
//! (`~/.local/share/Trash`) and one on each other drive (`.Trash-<uid>` or
//! `.Trash/<uid>`), each a `files` folder with an `info` file per item. On
//! macOS (`~/.Trash`, and `.Trashes/<uid>` on each other volume) Finder
//! empties it: the Trash folder is protected by macOS, and Finder knows
//! every drive's.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(any(test, not(target_os = "macos")))]
/// The Trash folders of this account, those that exist. A drive's Trash is
/// taken only as the freedesktop.org Trash specification says, so another
/// account (or a disk made to fool it) cannot point it at someone's files:
/// `.Trash-<uid>`, or `.Trash/<uid>` in a sticky `.Trash`, each a real
/// folder (not a link) of this account's. In every Trash, `files` must be a
/// real folder and `info` and `expunged`, when there, too.
pub fn linux_folders(home_trash: Option<&Path>, drive_roots: &[String], uid: u32) -> Vec<PathBuf> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let real_folder = |path: &Path| fs::symlink_metadata(path).ok().filter(fs::Metadata::is_dir);
    let owned = |path: &Path| real_folder(path).is_some_and(|meta| meta.uid() == uid);
    let mut folders: Vec<PathBuf> = home_trash.map(Path::to_path_buf).into_iter().collect();
    for root in drive_roots {
        let root = Path::new(root);
        let own = root.join(format!(".Trash-{uid}"));
        if owned(&own) {
            folders.push(own);
        }
        let shared = root.join(".Trash");
        let sticky =
            real_folder(&shared).is_some_and(|meta| meta.permissions().mode() & 0o1000 != 0);
        let mine = shared.join(uid.to_string());
        if sticky && owned(&mine) {
            folders.push(mine);
        }
    }
    folders.retain(|folder| {
        real_folder(&folder.join("files")).is_some()
            && ["info", "expunged"].iter().all(|part| {
                fs::symlink_metadata(folder.join(part)).map_or_else(
                    |error| error.kind() == io::ErrorKind::NotFound,
                    |meta| meta.is_dir(),
                )
            })
    });
    folders.dedup();
    folders
}

#[cfg(any(test, not(target_os = "macos")))]
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

/// What macOS's Trash folders hold: their items (`.DS_Store` aside) and
/// bytes, and whether one that is there could not be read (`~/.Trash` needs
/// Full Disk Access).
#[cfg(any(test, target_os = "macos"))]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counted {
    pub items: u64,
    pub bytes: u64,
    pub unreadable: bool,
}

/// Each item is measured where it is, links not followed: a name holding
/// `*`, `?` or `%` is only a name.
#[cfg(any(test, target_os = "macos"))]
pub fn mac_count(folders: &[PathBuf]) -> Counted {
    let mut counted = Counted::default();
    for folder in folders {
        match fs::read_dir(folder) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    if entry.file_name() != ".DS_Store" {
                        counted.items += 1;
                        counted.bytes += tree_size(&entry.path());
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => counted.unreadable = true,
        }
    }
    counted
}

/// The Trash's items and bytes on macOS from what its folders held. When
/// one could not be read (`~/.Trash` needs Full Disk Access), Finder is
/// asked (`ask`), whose count covers every volume: the larger count, with
/// the bytes of the folders read; when Finder cannot answer either, the
/// folders' count if any.
#[cfg(any(test, target_os = "macos"))]
pub fn mac_total(
    counted: Counted,
    ask: impl FnOnce() -> io::Result<u64>,
) -> io::Result<(u64, u64)> {
    if !counted.unreadable {
        return Ok((counted.items, counted.bytes));
    }
    match ask() {
        Ok(items) => Ok((items.max(counted.items), counted.bytes)),
        Err(_) if counted.items > 0 => Ok((counted.items, counted.bytes)),
        Err(error) => Err(error),
    }
}

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

#[cfg(any(test, not(target_os = "macos")))]
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
mod tests {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    use super::*;

    fn owner(path: &Path) -> u32 {
        fs::metadata(path).unwrap().uid()
    }

    fn sticky(path: &Path) {
        fs::set_permissions(path, fs::Permissions::from_mode(0o1777)).unwrap();
    }

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
    fn macos_trash_items_are_measured_by_their_own_names() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join(".Trash");
        fs::create_dir_all(trash.join("app/Contents")).unwrap();
        fs::write(trash.join("*draft*.txt"), b"123").unwrap();
        fs::write(trash.join("my draft.txt"), b"12345").unwrap();
        fs::write(trash.join("%HOME%x"), b"1234567").unwrap();
        fs::write(trash.join("back\\slash"), b"12").unwrap();
        fs::write(trash.join(".DS_Store"), [0u8; 100]).unwrap();
        fs::write(trash.join("app/Contents/bin"), [0u8; 11]).unwrap();
        let usb = dir.path().join("Volumes/USB/.Trashes/501");
        fs::create_dir_all(&usb).unwrap();
        fs::write(usb.join("photo.jpg"), [0u8; 40]).unwrap();
        let missing = dir.path().join("Volumes/Other/.Trashes/501");

        let counted = mac_count(&[trash.clone(), usb, missing]);
        assert_eq!(
            counted,
            Counted {
                items: 6,
                bytes: 3 + 5 + 7 + 2 + 11 + 40,
                unreadable: false
            }
        );

        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let counted = mac_count(&[trash, locked.clone()]);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(counted.items, 5);
        if !crate::sys::is_elevated() {
            assert!(counted.unreadable, "a folder it may not list");
        }
    }

    /// ~/.Trash unread (no Full Disk Access) while a disk's Trash is read.
    #[test]
    fn macos_asks_finder_only_when_a_trash_could_not_be_read() {
        let read = |items, unreadable| Counted {
            items,
            bytes: 40,
            unreadable,
        };
        let finder_fails = || Err(io::Error::other("Finder could not do it"));
        let total = mac_total(read(1, false), || panic!("Finder asked"));
        assert_eq!(total.unwrap(), (1, 40));
        assert_eq!(mac_total(read(3, true), || Ok(13)).unwrap(), (13, 40));
        assert_eq!(mac_total(read(3, true), || Ok(0)).unwrap(), (3, 40));
        assert_eq!(mac_total(read(2, true), finder_fails).unwrap(), (2, 40));
        assert!(mac_total(read(0, true), finder_fails).is_err());
    }

    #[test]
    fn other_drives_trash_folders_are_found_by_user_id() {
        let dir = tempfile::tempdir().unwrap();
        let uid = owner(dir.path());
        let root = dir.path().to_string_lossy().into_owned();
        fs::create_dir_all(dir.path().join(format!(".Trash-{uid}/files"))).unwrap();
        fs::create_dir_all(dir.path().join(format!(".Trash/{uid}/files"))).unwrap();
        fs::create_dir_all(dir.path().join(format!(".Trash-{}/files", uid + 1))).unwrap();
        sticky(&dir.path().join(".Trash"));
        let folders = linux_folders(None, &[root], uid);
        assert_eq!(folders.len(), 2);
    }

    /// The freedesktop.org rules: another account (or a crafted disk) must
    /// not be able to point a drive's Trash at someone's files.
    #[test]
    fn a_drives_trash_that_is_a_link_or_not_this_accounts_is_passed_over() {
        let dir = tempfile::tempdir().unwrap();
        let uid = owner(dir.path());
        let victim = dir.path().join("victim");
        for part in ["files", "info"] {
            fs::create_dir_all(victim.join(part)).unwrap();
            fs::write(victim.join(part).join("precious.txt"), b"keep").unwrap();
        }
        let drive = |name: &str| {
            let root = dir.path().join(name);
            fs::create_dir(&root).unwrap();
            root
        };
        let mine = format!(".Trash-{uid}");
        // .Trash-<uid> is a link.
        let linked = drive("linked");
        symlink(&victim, linked.join(&mine)).unwrap();
        // .Trash is not sticky.
        let loose = drive("loose");
        fs::create_dir_all(loose.join(format!(".Trash/{uid}/files"))).unwrap();
        // .Trash is a link to a sticky folder.
        let shared = dir.path().join("shared");
        fs::create_dir_all(shared.join(format!("{uid}/files"))).unwrap();
        sticky(&shared);
        let linked_shared = drive("linked-shared");
        symlink(&shared, linked_shared.join(".Trash")).unwrap();
        // files or info inside a real .Trash-<uid> is a link.
        let linked_files = drive("linked-files");
        fs::create_dir(linked_files.join(&mine)).unwrap();
        symlink(victim.join("files"), linked_files.join(&mine).join("files")).unwrap();
        let linked_info = drive("linked-info");
        fs::create_dir_all(linked_info.join(&mine).join("files")).unwrap();
        symlink(victim.join("info"), linked_info.join(&mine).join("info")).unwrap();
        let roots: Vec<String> = [
            linked,
            loose.clone(),
            linked_shared,
            linked_files,
            linked_info,
        ]
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();

        empty(&linux_folders(None, &roots, uid)).unwrap();
        assert!(victim.join("files/precious.txt").exists());
        assert!(victim.join("info/precious.txt").exists());
        assert_eq!(linux_folders(None, &roots, uid), Vec::<PathBuf>::new());

        // Someone else's: this account made it, another one is asked for.
        let theirs = drive("theirs");
        fs::create_dir_all(theirs.join(format!(".Trash-{}/files", uid + 1))).unwrap();
        let theirs = [theirs.to_string_lossy().into_owned()];
        assert!(linux_folders(None, &theirs, uid + 1).is_empty());

        // Made sticky, the loose .Trash/<uid> is a real Trash.
        sticky(&loose.join(".Trash"));
        assert_eq!(
            linux_folders(None, &roots, uid),
            vec![loose.join(".Trash").join(uid.to_string())]
        );
    }
}
