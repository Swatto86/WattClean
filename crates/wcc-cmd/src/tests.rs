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
    let code = execute(command, system, data_dir, &mut out).unwrap();
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
    let temp = system
        .drive_c()
        .join(r"Users\Tester\AppData\Local\Temp\setup-1234.tmp");
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
    let secret = system.drive_c().join(r"Users\Tester\Documents\Secret");
    let (code, text) = output(&Command::Shred(vec![secret.clone()]), &system, dir.path());
    assert_eq!(code, 0, "{text}");
    assert!(text.starts_with("Shredded 1 files and 1 folders"), "{text}");
    assert!(!secret.exists());
}

#[test]
fn sizes_read_simply() {
    assert_eq!(human(10), "10 bytes");
    assert_eq!(human(1536), "1.5 KB");
    assert_eq!(human(3 << 30), "3.0 GB");
}
