use super::*;
use wcc_core::fake::FakeSystem;

fn args(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

#[test]
fn arguments_read_like_bleachbits() {
    assert_eq!(parse(&args("--list-cleaners")), Ok(Command::List));
    assert_eq!(parse(&args("")), Ok(Command::Help));
    assert_eq!(
        parse(&args("--clean -o firefox.* chrome.cache")),
        Ok(Command::Run {
            mode: Mode::Clean,
            preset: false,
            all_but_warning: false,
            overwrite: true,
            keys: vec!["firefox.*".into(), "chrome.cache".into()],
        })
    );
    assert!(parse(&args("--preview")).is_err(), "nothing named");
    assert!(parse(&args("--preview --preset")).is_ok());
    assert_eq!(
        parse(&args(r"--shred C:\a C:\b")),
        Ok(Command::Shred(vec![
            PathBuf::from(r"C:\a"),
            PathBuf::from(r"C:\b")
        ]))
    );
    assert!(parse(&args("--shred")).is_err());
    assert!(parse(&args(r"--wipe-free-space C:\ D:\")).is_err());
    assert!(parse(&args("--bogus")).is_err());
    assert!(parse(&args("--list-cleaners --clean")).is_err());
}

fn machine() -> (tempfile::TempDir, FakeSystem) {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(dir.path()).unwrap();
    (dir, system)
}

fn output(command: &Command, system: &FakeSystem, data_dir: &Path) -> (i32, String) {
    let mut out = Vec::new();
    let code = execute(command, system, data_dir, &AtomicBool::new(false), &mut out).unwrap();
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn preview_prints_every_item_and_a_summary_then_clean_deletes() {
    let (dir, system) = machine();
    let preview = parse(&args("--preview system.temp vlc.*")).unwrap();
    let (code, text) = output(&preview, &system, dir.path());
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("setup-1234.tmp"), "{text}");
    assert!(text.contains("Disk space to be recovered:"), "{text}");
    assert!(
        text.contains("vlc-qt-interface.ini"),
        "the ini edit is listed: {text}"
    );
    let temp = wcc_core::paths::join(
        &system.drive_c(),
        r"Users\Tester\AppData\Local\Temp\setup-1234.tmp",
    );
    assert!(temp.exists());
    let clean = parse(&args("--clean system.temp")).unwrap();
    let (code, text) = output(&clean, &system, dir.path());
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Disk space recovered:"));
    assert!(!temp.exists());
}

#[test]
fn listing_and_bad_names() {
    let (dir, system) = machine();
    let (code, text) = output(&Command::List, &system, dir.path());
    assert_eq!(code, 0);
    assert!(text.lines().any(|l| l == "chrome.cache"));
    assert!(
        text.lines().any(|l| l == "northwind.cache"),
        "custom cleaners are listed"
    );
    let (code, text) = output(
        &parse(&args("--preview nope.cache")).unwrap(),
        &system,
        dir.path(),
    );
    assert_eq!(code, 2);
    assert!(text.contains("no option nope.cache"));
}

#[test]
fn shredding_from_the_command_line() {
    let (dir, system) = machine();
    let secret = wcc_core::paths::join(&system.drive_c(), r"Users\Tester\Documents\Secret");
    let (code, text) = output(&Command::Shred(vec![secret.clone()]), &system, dir.path());
    assert_eq!(code, 0, "{text}");
    assert!(text.starts_with("Shredded 1 files and 1 folders"), "{text}");
    assert!(!secret.exists());
}

#[test]
fn a_chosen_name_with_a_percent_sign_is_shredded() {
    let (dir, system) = machine();
    let file = wcc_core::paths::join(&system.drive_c(), r"Users\Tester\Documents\100% done.txt");
    std::fs::write(&file, b"x").unwrap();
    let (code, text) = output(&Command::Shred(vec![file.clone()]), &system, dir.path());
    assert_eq!(code, 0, "{text}");
    assert!(!file.exists());
}

fn wipe_folders(folder: &Path) -> Vec<String> {
    std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with("WattClean-wipe-"))
        .collect()
}

#[test]
fn a_wipe_removes_an_earlier_one_left_behind_and_its_own() {
    let (dir, system) = machine();
    let folder = system.drive_c();
    let leftover = folder.join("WattClean-wipe-0123456789abcdef");
    std::fs::create_dir(&leftover).unwrap();
    std::fs::write(leftover.join("fill00000.bin"), [0u8; 4096]).unwrap();
    let (code, text) = output(&Command::Wipe(folder.clone()), &system, dir.path());
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Wrote and removed 3.0 MB"), "{text}");
    assert!(wipe_folders(&folder).is_empty());
}

#[test]
fn ctrl_c_stops_a_wipe_and_gives_the_space_back() {
    let (dir, system) = machine();
    let folder = system.drive_c();
    let mut out = Vec::new();
    let stop = AtomicBool::new(true);
    let code = execute(
        &Command::Wipe(folder.clone()),
        &system,
        dir.path(),
        &stop,
        &mut out,
    )
    .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("Stopped after writing 0 bytes"), "{text}");
    assert!(wipe_folders(&folder).is_empty());
}

#[test]
fn sizes_read_simply() {
    assert_eq!(human(10), "10 bytes");
    assert_eq!(human(1536), "1.5 KB");
    assert_eq!(human(3 << 30), "3.0 GB");
}
