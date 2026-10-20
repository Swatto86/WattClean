use std::fs;
use std::time::SystemTime;

use super::*;
use crate::fake::FakeSystem;

const NEVER: fn() -> bool = || false;

fn planner<'a>(system: &'a FakeSystem, guard: &'a Guard, custom: &'a [CustomPath]) -> Planner<'a> {
    Planner {
        system,
        guard,
        custom_paths: custom,
        now: SystemTime::now(),
        cancelled: &NEVER,
    }
}

fn planned(ops: &[Op]) -> Vec<(String, ItemKind)> {
    let mut items: Vec<_> = ops
        .iter()
        .map(|op| {
            let item = op.item();
            (item.path, item.kind)
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
}

/// A custom path is the file or folder the person picked, whatever its
/// name holds: a `%NAME%` there is part of the name, not a variable.
#[test]
fn a_custom_path_with_percent_signs_is_not_expanded() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(dir.path()).unwrap();
    let file = dir.path().join("50%TEMP%.txt");
    fs::write(&file, b"x").unwrap();
    let custom = [CustomPath {
        path: file.to_string_lossy().to_string(),
        folder: false,
    }];
    let guard = Guard::default();
    let (ops, problems) =
        planner(&system, &guard, &custom).plan(&Action::CustomPaths, &mut HashSet::new());
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(planned(&ops), vec![(show(&file), ItemKind::File)]);
}

/// Names macOS and Linux allow, which planning must not read as wildcards
/// or separators.
#[cfg(unix)]
#[test]
fn custom_paths_are_taken_as_written() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(&dir.path().join("sandbox")).unwrap();
    let at = |name: &str| dir.path().join(name);
    for folder in ["Why?", "Whys", "a"] {
        fs::create_dir(at(folder)).unwrap();
    }
    for file in ["Why?/a.txt", "Whys/thesis.txt", "a/b.txt", "a\\b.txt"] {
        fs::write(at(file), b"x").unwrap();
    }
    let custom = [
        CustomPath {
            path: show(&at("Why?")),
            folder: true,
        },
        CustomPath {
            path: show(&at("a\\b.txt")),
            folder: false,
        },
    ];
    let guard = Guard::default();
    let (ops, problems) =
        planner(&system, &guard, &custom).plan(&Action::CustomPaths, &mut HashSet::new());
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        planned(&ops),
        vec![
            (show(&at("Why?")), ItemKind::Folder),
            (show(&at("Why?/a.txt")), ItemKind::File),
            (show(&at("a\\b.txt")), ItemKind::File),
        ]
    );
}

/// Emptying or editing a file through a link would change the link's
/// target, which the guard never saw: links are only ever removed.
#[cfg(unix)]
#[test]
fn in_place_edits_are_never_planned_on_links() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(&dir.path().join("sandbox")).unwrap();
    let mut database = b"SQLite format 3\0".to_vec();
    database.resize(4096, 0);
    let targets: [(&str, &[u8]); 4] = [
        ("log", b"kept text"),
        ("json", br#"{"x":1}"#),
        ("ini", b"[s]\nk=1\n"),
        ("db", &database),
    ];
    for (name, contents) in targets {
        let target = dir.path().join(format!("target.{name}"));
        fs::write(&target, contents).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(format!("link.{name}"))).unwrap();
    }
    let spec = |name: &str, search: Search| FileSpec::new(show(&dir.path().join(name)), search);
    let actions = [
        Action::Truncate(spec("link.log", Search::File)),
        Action::Truncate(spec("link.*", Search::Glob)),
        Action::Json {
            files: spec("link.json", Search::File),
            address: "x".into(),
        },
        Action::Ini {
            files: spec("link.ini", Search::File),
            section: "s".into(),
            parameter: Some("k".into()),
        },
        Action::Sqlite {
            files: spec("link.db", Search::File),
            op: crate::model::SqliteOp::Vacuum,
        },
    ];
    let guard = Guard::default();
    let planner = planner(&system, &guard, &[]);
    for action in &actions {
        let (ops, _) = planner.plan(action, &mut HashSet::new());
        assert!(ops.is_empty(), "{action:?} planned {ops:?}");
    }
    let (ops, _) = planner.plan(
        &Action::Delete(spec("link.log", Search::File)),
        &mut HashSet::new(),
    );
    assert_eq!(
        ops,
        vec![Op::DeleteFile {
            path: dir.path().join("link.log"),
            size: 0
        }],
        "the link itself may still go, and is no bigger than nothing"
    );
}
