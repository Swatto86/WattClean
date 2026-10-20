use super::*;

const ONE_NAME: fn(&Path) -> io::Result<u32> = |_| Ok(1);

#[test]
fn shredding_overwrites_and_removes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("secret.txt");
    fs::write(&file, b"top secret").unwrap();
    shred_file(&file, &ONE_NAME).unwrap();
    assert!(!file.exists());
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        0,
        "no renamed copy left"
    );
}

#[test]
fn shredding_folders_takes_their_contents() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Private");
    fs::create_dir_all(folder.join("deeper")).unwrap();
    fs::write(folder.join("a.txt"), b"aaaa").unwrap();
    fs::write(folder.join("deeper").join("b.txt"), b"bb").unwrap();
    let loose = dir.path().join("loose.txt");
    fs::write(&loose, b"c").unwrap();
    let report = shred_paths(
        &[folder.clone(), loose.clone()],
        &Guard::default(),
        &ONE_NAME,
        &|| false,
        &|_, _| {},
    );
    assert_eq!((report.files, report.folders, report.bytes), (3, 2, 7));
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert!(!folder.exists() && !loose.exists());
}

#[test]
fn kept_paths_are_not_shredded() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("keep.txt");
    fs::write(&file, b"x").unwrap();
    let guard = Guard::new(&[], std::slice::from_ref(&file));
    let report = shred_paths(
        std::slice::from_ref(&file),
        &guard,
        &ONE_NAME,
        &|| false,
        &|_, _| {},
    );
    assert_eq!(report.files, 0);
    assert_eq!(report.problems.len(), 1);
    assert!(file.exists());
}

#[test]
fn wiping_writes_up_to_the_limit_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let seen = AtomicU64::new(0);
    let report = wipe_free_space(dir.path(), Some(3 * CHUNK as u64 + 10), &|| false, &|n| {
        seen.store(n, Ordering::Relaxed);
    })
    .unwrap();
    assert_eq!(report.written, 3 * CHUNK as u64 + 10);
    assert_eq!(seen.load(Ordering::Relaxed), report.written);
    assert!(!report.cancelled);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn a_cancelled_wipe_still_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let report = wipe_free_space(dir.path(), Some(1 << 30), &|| true, &|_| {}).unwrap();
    assert!(report.cancelled);
    assert_eq!(report.written, 0);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn a_file_that_cannot_be_removed_keeps_its_own_name() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("secret.txt");
    fs::write(&file, b"top secret").unwrap();
    let refused = shred_with(&file, &ONE_NAME, &|_| Err(io::Error::from_raw_os_error(32)));
    assert_eq!(refused.unwrap_err().raw_os_error(), Some(32));
    assert_eq!(fs::read(&file).unwrap(), b"", "the data was overwritten");
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec!["secret.txt"], "no random-named file left");
}

#[cfg(windows)]
#[test]
fn a_filler_folder_held_open_for_a_moment_is_still_removed() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("WattClean-wipe-test");
    fs::create_dir(&work).unwrap();
    let held = work.join("fill00000.bin");
    fs::write(&held, b"0000").unwrap();
    // No sharing, as a virus scanner reading the file just written.
    let scanner = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held)
        .unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        drop(scanner);
    });
    let removed = remove_filler(&work);
    release.join().unwrap();
    removed.unwrap();
    assert!(!work.exists());
}

#[test]
fn a_file_whose_data_has_other_names_is_not_shredded() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cache.bin");
    let other = dir.path().join("elsewhere.bin");
    fs::write(&file, b"shared data").unwrap();
    fs::hard_link(&file, &other).unwrap();
    let report = shred_paths(
        std::slice::from_ref(&file),
        &Guard::default(),
        &|_| Ok(2),
        &|| false,
        &|_, _| {},
    );
    assert_eq!(report.files, 0);
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].message.contains("hard links"));
    assert_eq!(fs::read(&other).unwrap(), b"shared data");
    assert_eq!(fs::read(&file).unwrap(), b"shared data");
}

#[test]
fn what_was_not_fully_shredded_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Private");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("a.txt"), b"a").unwrap();
    fs::write(folder.join("keep.txt"), b"k").unwrap();
    let loose = dir.path().join("loose.txt");
    fs::write(&loose, b"c").unwrap();
    let missing = dir.path().join("gone.txt");
    let guard = Guard::new(&[], &[folder.join("keep.txt")]);
    let report = shred_paths(
        &[folder.clone(), loose.clone(), missing],
        &guard,
        &ONE_NAME,
        &|| false,
        &|_, _| {},
    );
    assert_eq!(report.files, 2, "{:?}", report.problems);
    assert_eq!(report.left, vec![folder.to_string_lossy().to_string()]);
}

/// Names macOS and Linux allow, which the search must not read as
/// wildcards or separators: only what was chosen is shredded.
#[cfg(unix)]
#[test]
fn chosen_paths_are_taken_literally() {
    let dir = tempfile::tempdir().unwrap();
    let at = |name: &str| dir.path().join(name);
    let names = [
        "Report?.pdf",
        "Report1.pdf",
        "ReportZ.pdf",
        "a\\b.txt",
        "50%TEMP%.txt",
    ];
    for name in names {
        fs::write(at(name), b"x").unwrap();
    }
    for folder in ["Why?", "Whys", "a"] {
        fs::create_dir(at(folder)).unwrap();
    }
    fs::write(at("Why?").join("a.txt"), b"x").unwrap();
    fs::write(at("Whys").join("thesis.txt"), b"x").unwrap();
    fs::write(at("a").join("b.txt"), b"x").unwrap();
    let report = shred_paths(
        &[
            at("Report?.pdf"),
            at("Why?"),
            at("a\\b.txt"),
            at("50%TEMP%.txt"),
        ],
        &Guard::default(),
        &ONE_NAME,
        &|| false,
        &|_, _| {},
    );
    assert_eq!((report.files, report.folders), (4, 1), "{report:?}");
    for chosen in ["Report?.pdf", "Why?", "a\\b.txt", "50%TEMP%.txt"] {
        assert!(!at(chosen).exists(), "{chosen} was chosen");
    }
    for kept in ["Report1.pdf", "ReportZ.pdf", "Whys/thesis.txt", "a/b.txt"] {
        assert!(at(kept).exists(), "{kept} was never chosen");
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// A folder that will not let a file be renamed will not let it be removed
/// either: the file is left exactly as it was, not emptied in place.
#[cfg(unix)]
#[test]
fn a_file_its_folder_will_not_let_go_is_left_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("ro");
    fs::create_dir(&folder).unwrap();
    let file = folder.join("data.txt");
    fs::write(&file, b"precious contents").unwrap();
    set_mode(&file, 0o444);
    set_mode(&folder, 0o555);
    let result = shred_file(&file, &ONE_NAME);
    let (contents, file_mode) = (fs::read(&file), mode(&file));
    set_mode(&folder, 0o755);
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(contents.unwrap(), b"precious contents");
    assert_eq!(file_mode, 0o444, "never made writable by others");
}

#[cfg(unix)]
#[test]
fn a_read_only_file_is_shredded() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("secret.txt");
    fs::write(&file, b"top secret").unwrap();
    set_mode(&file, 0o400);
    shred_file(&file, &ONE_NAME).unwrap();
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_removed_gets_its_name_and_mode_back() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("secret.txt");
    fs::write(&file, b"top secret").unwrap();
    set_mode(&file, 0o444);
    let while_shredded = std::cell::Cell::new(0);
    let refused = shred_with(&file, &ONE_NAME, &|renamed| {
        while_shredded.set(mode(renamed));
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    });
    assert!(refused.is_err());
    assert_eq!(while_shredded.get(), 0o644, "only the owner may write");
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec!["secret.txt"], "no random-named file left");
    assert_eq!(mode(&file), 0o444);
    assert_eq!(fs::read(&file).unwrap(), b"", "the data was overwritten");
}

/// A file swapped for a link after it was looked at is refused, and the
/// link's target is left alone.
#[cfg(unix)]
#[test]
fn opening_for_an_overwrite_refuses_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("elsewhere.txt");
    fs::write(&target, b"not this one").unwrap();
    let link = dir.path().join("swapped.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(ops::open_to_overwrite(&link).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"not this one");
}

/// A link chosen with a separator after its name (as a shell completes a
/// folder) is still only the link: the system would otherwise follow it
/// and shred the folder it points to, which the guard never saw.
#[cfg(unix)]
#[test]
fn a_chosen_link_written_with_a_trailing_separator_is_only_unlinked() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("Keep");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("precious.txt"), b"keep").unwrap();
    let link = dir.path().join("Link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let chosen = PathBuf::from(format!("{}/", link.display()));
    let report = shred_paths(
        std::slice::from_ref(&chosen),
        &Guard::new(&[], std::slice::from_ref(&target)),
        &ONE_NAME,
        &|| false,
        &|_, _| {},
    );
    assert_eq!(fs::read(target.join("precious.txt")).unwrap(), b"keep");
    assert!(fs::symlink_metadata(&link).is_err(), "{report:?}");
    assert!(report.left.is_empty(), "{report:?}");
}
