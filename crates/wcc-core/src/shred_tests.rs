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
    fs::write(&file, b"").unwrap();
    let refused = unlink_renamed(&file, &|_| Err(io::Error::from_raw_os_error(32)));
    assert_eq!(refused.unwrap_err().raw_os_error(), Some(32));
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
    let scanner = OpenOptions::new()
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
