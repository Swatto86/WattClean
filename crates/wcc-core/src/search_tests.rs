use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::*;
use crate::model::{FileSpec, Kind, Search};
use crate::paths::Guard;

fn no_vars(_: &str) -> Vec<String> {
    Vec::new()
}

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for file in [
        r"App\Cache\a.bin",
        r"App\Cache\sub\b.bin",
        r"App\Cache\sub\keep.txt",
        r"App\Logs\one.log",
        r"App\Logs\two.log",
        r"App\Logs\notes.txt",
        r"Other\Profile1\Cache\c.bin",
        r"Other\Profile2\Cache\d.bin",
        r"Other\Profile2\Settings\e.bin",
    ] {
        let path = crate::paths::join(root, file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"12345").unwrap();
    }
    dir
}

fn run(spec: &FileSpec, guard: &Guard) -> Vec<(String, Kind)> {
    let filter = Filter::new(spec, &no_vars, SystemTime::now()).unwrap();
    let never = || false;
    let searcher = Searcher {
        guard,
        cancelled: &never,
    };
    let mut out = Findings::default();
    searcher.find(spec, &filter, &no_vars, &mut out);
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    out.found
        .into_iter()
        .map(|f| (f.path.to_string_lossy().replace('\\', "/"), f.kind))
        .collect()
}

fn rel(root: &Path, found: Vec<(String, Kind)>) -> Vec<(String, Kind)> {
    let prefix = format!("{}/", root.to_string_lossy().replace('\\', "/"));
    found
        .into_iter()
        .map(|(p, k)| (p.strip_prefix(&prefix).unwrap_or(&p).to_string(), k))
        .collect()
}

fn path(root: &Path, rel: &str) -> String {
    crate::paths::join(root, rel).to_string_lossy().to_string()
}

#[test]
fn walk_files_lists_every_file_below() {
    let dir = tree();
    let spec = FileSpec::new(path(dir.path(), r"App\Cache"), Search::WalkFiles);
    let mut found = rel(dir.path(), run(&spec, &Guard::default()));
    found.sort();
    assert_eq!(
        found,
        vec![
            ("App/Cache/a.bin".to_string(), Kind::File),
            ("App/Cache/sub/b.bin".to_string(), Kind::File),
            ("App/Cache/sub/keep.txt".to_string(), Kind::File),
        ]
    );
}

#[test]
fn walk_top_lists_folders_after_their_contents_and_the_root_last() {
    let dir = tree();
    let spec = FileSpec::new(path(dir.path(), r"App\Cache"), Search::WalkTop);
    let found = rel(dir.path(), run(&spec, &Guard::default()));
    let position = |p: &str| found.iter().position(|(x, _)| x == p).unwrap();
    assert!(position("App/Cache/sub/b.bin") < position("App/Cache/sub"));
    assert_eq!(found.last().unwrap(), &("App/Cache".to_string(), Kind::Dir));
    assert_eq!(found.len(), 5);
}

#[test]
fn a_folder_with_anything_left_in_it_is_not_listed() {
    let dir = tree();
    let mut spec = FileSpec::new(path(dir.path(), r"App\Cache"), Search::WalkTop);
    spec.regex = Some(r"\.bin$".into());
    let found = rel(dir.path(), run(&spec, &Guard::default()));
    assert_eq!(
        found,
        vec![
            ("App/Cache/a.bin".to_string(), Kind::File),
            ("App/Cache/sub/b.bin".to_string(), Kind::File),
        ]
    );
}

#[test]
fn wildcards_match_in_any_component() {
    let dir = tree();
    let spec = FileSpec::new(path(dir.path(), r"Other\*\Cache"), Search::WalkFiles);
    let mut found = rel(dir.path(), run(&spec, &Guard::default()));
    found.sort();
    assert_eq!(
        found,
        vec![
            ("Other/Profile1/Cache/c.bin".to_string(), Kind::File),
            ("Other/Profile2/Cache/d.bin".to_string(), Kind::File),
        ]
    );
    let spec = FileSpec::new(path(dir.path(), r"App\Logs\*.LOG"), Search::Glob);
    assert_eq!(rel(dir.path(), run(&spec, &Guard::default())).len(), 2);
}

#[test]
fn name_wildcards_and_exclusions_filter() {
    let dir = tree();
    let mut spec = FileSpec::new(path(dir.path(), "App"), Search::WalkFiles);
    spec.names = Some("*.log;*.txt".into());
    spec.exclude = vec![path(dir.path(), r"App\Cache")];
    let mut found = rel(dir.path(), run(&spec, &Guard::default()));
    found.sort();
    let names: Vec<_> = found.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        names,
        vec!["App/Logs/notes.txt", "App/Logs/one.log", "App/Logs/two.log"]
    );
}

#[test]
fn the_keep_list_hides_a_subtree_and_keeps_its_parents() {
    let dir = tree();
    let guard = Guard::new(&[], &[PathBuf::from(path(dir.path(), r"App\Cache\sub"))]);
    let spec = FileSpec::new(path(dir.path(), r"App\Cache"), Search::WalkTop);
    let found = rel(dir.path(), run(&spec, &guard));
    assert_eq!(found, vec![("App/Cache/a.bin".to_string(), Kind::File)]);
}

#[test]
fn the_search_file_finds_a_file_or_folder_by_exact_path() {
    let dir = tree();
    let spec = FileSpec::new(path(dir.path(), r"App\Logs\one.log"), Search::File);
    assert_eq!(run(&spec, &Guard::default()).len(), 1);
    let spec = FileSpec::new(path(dir.path(), r"App\Logs\missing.log"), Search::File);
    assert!(run(&spec, &Guard::default()).is_empty());
    let mut spec = FileSpec::new(path(dir.path(), r"App\Logs"), Search::File);
    spec.kind = Some(Kind::File);
    assert!(run(&spec, &Guard::default()).is_empty());
}

#[test]
fn a_bad_regex_is_reported() {
    let mut spec = FileSpec::new("x", Search::File);
    spec.regex = Some("(".into());
    assert!(Filter::new(&spec, &no_vars, SystemTime::now()).is_err());
}

#[cfg(windows)]
#[test]
fn links_are_listed_but_never_followed() {
    let dir = tree();
    let target = dir.path().join("Other");
    let link = crate::paths::join(dir.path(), r"App\Cache\link");
    junction(&link, &target);
    let spec = FileSpec::new(path(dir.path(), r"App\Cache"), Search::WalkAll);
    let found = rel(dir.path(), run(&spec, &Guard::default()));
    assert!(found.contains(&("App/Cache/link".to_string(), Kind::Dir)));
    assert!(!found.iter().any(|(p, _)| p.contains("Profile1")));
    // A clean removes folders with remove_dir: only the link goes.
    crate::exec::remove_dir(&link).unwrap();
    assert!(!link.exists());
    assert!(crate::paths::join(&target, r"Profile1\Cache\c.bin").exists());
}

/// A junction, which Windows lets any user create (a symlink needs rights).
#[cfg(windows)]
pub(crate) fn junction(link: &Path, target: &Path) {
    let status = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "mklink /J failed");
}

#[test]
fn walk_all_with_a_name_filter_leaves_folders_alone() {
    let dir = tree();
    fs::create_dir_all(crate::paths::join(dir.path(), r"App\Reports\WATCHDOG")).unwrap();
    fs::create_dir_all(crate::paths::join(dir.path(), r"App\Reports\Full")).unwrap();
    fs::write(
        crate::paths::join(dir.path(), r"App\Reports\Full\x.dmp"),
        b"1",
    )
    .unwrap();
    fs::write(crate::paths::join(dir.path(), r"App\Reports\top.dmp"), b"1").unwrap();
    let mut spec = FileSpec::new(path(dir.path(), r"App\Reports"), Search::WalkAll);
    spec.names = Some("*.dmp".into());
    let mut found = rel(dir.path(), run(&spec, &Guard::default()));
    found.sort();
    assert_eq!(
        found,
        vec![
            ("App/Reports/Full/x.dmp".to_string(), Kind::File),
            ("App/Reports/top.dmp".to_string(), Kind::File),
        ]
    );
    // Without a name filter the emptied folders still go.
    let spec = FileSpec::new(path(dir.path(), r"App\Reports"), Search::WalkAll);
    let found = rel(dir.path(), run(&spec, &Guard::default()));
    assert!(found.contains(&("App/Reports/WATCHDOG".to_string(), Kind::Dir)));
}

#[test]
fn an_exclusion_that_cannot_be_expanded_refuses_the_search() {
    let mut spec = FileSpec::new("x", Search::WalkFiles);
    spec.exclude = vec![r"%NoSuchVariable%\Keep".into()];
    assert!(Filter::new(&spec, &no_vars, SystemTime::now()).is_err());
    // OneDrive is simply absent on many PCs: nothing to exclude there.
    spec.exclude = vec![r"%ONEDRIVE%".into()];
    assert!(Filter::new(&spec, &no_vars, SystemTime::now()).is_ok());
}

#[test]
fn a_wildcard_exclusion_skips_matching_folders_at_any_depth() {
    let dir = tree();
    for file in [
        r"Proj\node_modules\x\Thumbs.db",
        r"Proj\deep\node_modules\Thumbs.db",
        r"Proj\Thumbs.db",
    ] {
        let path = crate::paths::join(dir.path(), file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"t").unwrap();
    }
    let mut spec = FileSpec::new(path(dir.path(), "Proj"), Search::WalkFiles);
    spec.regex = Some(r"^Thumbs\.db$".into());
    spec.exclude = vec![path(dir.path(), r"Proj\*\node_modules")];
    let found = rel(dir.path(), run(&spec, &Guard::default()));
    assert_eq!(found, vec![("Proj/Thumbs.db".to_string(), Kind::File)]);
}

fn projects() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in [
        r"Code\app\Cargo.toml",
        r"Code\app\src\main.rs",
        r"Code\app\target\debug\app.exe",
        r"Code\app\target\debug\deps\app.d",
        r"Code\nested\deep\tool\Cargo.toml",
        r"Code\nested\deep\tool\target\release\tool.exe",
        r"Code\notes\target\plan.txt",
        r"Code\web\node_modules\pkg\Cargo.toml",
        r"Code\web\node_modules\pkg\target\x.bin",
        r"Code\kept\Cargo.toml",
        r"Code\kept\target\y.bin",
    ] {
        let path = crate::paths::join(dir.path(), file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"build").unwrap();
    }
    dir
}

#[test]
fn find_dirs_takes_named_folders_beside_their_marker_whole() {
    let dir = projects();
    let guard = Guard::new(&[], &[PathBuf::from(path(dir.path(), r"Code\kept"))]);
    let mut spec = FileSpec::new(path(dir.path(), "Code"), Search::FindDirs);
    spec.names = Some("target".into());
    spec.beside = Some("Cargo.toml".into());
    spec.exclude = vec![path(dir.path(), r"Code\*\node_modules")];
    let found = rel(dir.path(), run(&spec, &guard));
    let mut paths: Vec<_> = found.iter().map(|(p, _)| p.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        vec![
            "Code/app/target",
            "Code/app/target/debug",
            "Code/app/target/debug/app.exe",
            "Code/app/target/debug/deps",
            "Code/app/target/debug/deps/app.d",
            "Code/nested/deep/tool/target",
            "Code/nested/deep/tool/target/release",
            "Code/nested/deep/tool/target/release/tool.exe",
        ]
    );
    let position = |p: &str| found.iter().position(|(x, _)| x == p).unwrap();
    assert!(position("Code/app/target/debug/app.exe") < position("Code/app/target"));
}

#[test]
fn find_dirs_without_a_marker_takes_every_folder_of_that_name() {
    let dir = projects();
    let mut spec = FileSpec::new(path(dir.path(), r"Code\notes"), Search::FindDirs);
    spec.names = Some("target".into());
    let found = rel(dir.path(), run(&spec, &Guard::default()));
    assert_eq!(
        found,
        vec![
            ("Code/notes/target/plan.txt".to_string(), Kind::File),
            ("Code/notes/target".to_string(), Kind::Dir),
        ]
    );
}

/// Sockets and pipes in a temporary folder belong to running programs.
#[cfg(unix)]
#[test]
fn sockets_and_pipes_are_never_listed() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("session.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let status = std::process::Command::new("mkfifo")
        .arg(dir.path().join("pipe"))
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(dir.path().join("old.tmp"), b"x").unwrap();
    let spec = FileSpec::new(dir.path().to_string_lossy(), Search::WalkAll);
    let found = rel(dir.path(), run(&spec, &Guard::default()));
    assert_eq!(found, vec![("old.tmp".to_string(), Kind::File)]);
}

/// A folder the system refuses to list is reported, not taken for empty.
#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_listed_is_a_problem_not_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("Locked");
    fs::create_dir_all(locked.join("Profile")).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    // As root everything can be listed: nothing to show then.
    let listable = fs::read_dir(&locked).is_ok();
    let spec = FileSpec::new(format!("{}/*/History", locked.display()), Search::Glob);
    let filter = Filter::new(&spec, &no_vars, SystemTime::now()).unwrap();
    let never = || false;
    let searcher = Searcher {
        guard: &Guard::default(),
        cancelled: &never,
    };
    let mut out = Findings::default();
    searcher.find(&spec, &filter, &no_vars, &mut out);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if !listable {
        assert_eq!(out.problems.len(), 1, "{:?}", out.problems);
        assert_eq!(out.problems[0].path, locked);
    }
    // A folder that is simply not there is no problem.
    let missing = FileSpec::new(format!("{}/Nothing/*", dir.path().display()), Search::Glob);
    let mut none = Findings::default();
    searcher.find(&missing, &filter, &no_vars, &mut none);
    assert!(none.problems.is_empty());
}

#[test]
fn error_texts_fit_the_platform() {
    let text = |code| describe(&std::io::Error::from_raw_os_error(code));
    assert_eq!(text(32), "in use by another program");
    if cfg!(windows) {
        assert!(text(5).contains("administrator"));
    } else {
        assert!(
            !text(5).contains("administrator"),
            "EIO is not access denied"
        );
    }
    if cfg!(target_os = "macos") {
        assert!(text(1).contains("Full Disk Access"));
        assert!(
            !text(13).contains("Full Disk Access"),
            "EACCES is plain permissions"
        );
    }
}
