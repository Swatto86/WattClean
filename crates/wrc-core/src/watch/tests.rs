//! Changes beneath a watched key are logged with old and new data, and the
//! watch list survives a restart.

use std::time::{Duration, Instant};

use super::*;
use crate::RawValue;
use crate::fake::FakeSystem;
use crate::model::Hive;

fn wait_for(watcher: &Watcher, key: &RegPath, count: usize) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let events = watcher.events(key, 0).unwrap();
        if events.len() >= count || Instant::now() > deadline {
            return events;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn diff_reports_each_kind_of_change() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::empty(dir.path());
    let root = RegPath::new(Hive::CurrentUser, r"Software\Acme");
    system.put(
        &root,
        vec![
            RawValue::string("Keep", "1"),
            RawValue::string("Change", "a"),
            RawValue::string("Drop", "x"),
        ],
    );
    system.put(&root.child("Old"), vec![]);
    let before = snapshot(&system, &root).unwrap();
    system.delete_key_tree(&root.child("Old")).unwrap();
    system.put(&root.child("New"), vec![RawValue::dword("n", 1)]);
    system.put(&root, vec![RawValue::string("Change", "b")]);
    system.delete_value(&root, "Drop").unwrap();
    let after = snapshot(&system, &root).unwrap();
    let ops: Vec<(Op, String, Option<String>)> = diff(&before, &after)
        .into_iter()
        .map(|change| (change.op, change.key.name().to_owned(), change.value))
        .collect();
    assert_eq!(
        ops,
        vec![
            (Op::ValueChanged, "Acme".into(), Some("Change".into())),
            (Op::ValueDeleted, "Acme".into(), Some("Drop".into())),
            (Op::KeyCreated, "New".into(), None),
            (Op::ValueAdded, "New".into(), Some("n".into())),
            (Op::KeyDeleted, "Old".into(), None),
        ]
    );
}

#[test]
fn a_watch_logs_changes_and_its_list_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let system: Arc<FakeSystem> = Arc::new(FakeSystem::empty(&dir.path().join("machine")));
    let key = RegPath::new(Hive::CurrentUser, r"Software\Acme");
    system.put(&key, vec![]);
    let watcher = Watcher::start(Arc::clone(&system) as Arc<dyn System>, dir.path()).unwrap();
    watcher.add(key.clone()).unwrap();
    assert!(watcher.add(key.clone()).is_err(), "watched once only");
    system
        .write_value(&key, &RawValue::string("Mode", "fast"))
        .unwrap();
    let events = wait_for(&watcher, &key, 1);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].change.op, Op::ValueAdded);
    assert_eq!(events[0].change.new.as_deref(), Some("fast"));

    watcher.pause(&key, true).unwrap();
    system
        .write_value(&key, &RawValue::string("Mode", "slow"))
        .unwrap();
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(
        watcher.events(&key, 0).unwrap().len(),
        1,
        "paused watches log nothing"
    );
    watcher.stop_all();

    let again = Watcher::start(Arc::clone(&system) as Arc<dyn System>, dir.path()).unwrap();
    let listed = again.list();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].paused);
    assert_eq!(listed[0].events, 1, "the log came back with the list");
    again.pause(&key, false).unwrap();
    system.delete_value(&key, "Mode").unwrap();
    let events = wait_for(&again, &key, 2);
    assert_eq!(events[0].change.op, Op::ValueAdded);
    assert_eq!((events[1].seq, events[1].change.op), (2, Op::ValueDeleted));
    assert_eq!(events[1].change.old.as_deref(), Some("slow"));
    again.remove(&key).unwrap();
    assert!(again.list().is_empty());
    let logs = std::fs::read_dir(dir.path().join(log::FOLDER))
        .unwrap()
        .count();
    assert_eq!(logs, 0, "a key no longer watched keeps no log");
    again.stop_all();
}

#[test]
fn a_log_file_skips_damaged_lines_and_is_compacted() {
    let dir = tempfile::tempdir().unwrap();
    let key = RegPath::new(Hive::CurrentUser, r"Software\Acme");
    let change = |name: &str| Change {
        op: Op::ValueAdded,
        key: key.clone(),
        value: Some(name.into()),
        kind: Some("REG_SZ".into()),
        old: None,
        new: Some("x".repeat(100)),
    };
    let mut first = log::Log::fresh(dir.path(), &key).with_limit(1_000);
    for at in 0..20 {
        first.record(vec![explain::Found {
            change: change(&format!("v{at}")),
            actor: None,
            time: at,
        }]);
    }
    let folder = dir.path().join(log::FOLDER);
    let file = std::fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(
        std::fs::metadata(&file).unwrap().len() <= 1_000 + 300,
        "it was compacted"
    );
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str(
        "{ not json
",
    );
    std::fs::write(&file, text).unwrap();
    let again = log::Log::load(dir.path(), &key);
    assert_eq!(again.next, 20, "numbering carries on");
    assert!(!again.events.is_empty());
    assert!(
        again
            .events
            .iter()
            .all(|event| event.change.value.is_some())
    );
    let mut cleared = again;
    cleared.clear();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "");
}

#[test]
fn a_missing_or_huge_key_cannot_be_watched() {
    let dir = tempfile::tempdir().unwrap();
    let system: Arc<FakeSystem> = Arc::new(FakeSystem::empty(&dir.path().join("machine")));
    let watcher = Watcher::start(Arc::clone(&system) as Arc<dyn System>, dir.path()).unwrap();
    assert!(
        watcher
            .add(RegPath::new(Hive::CurrentUser, r"Software\Nothing"))
            .is_err()
    );
    let big = RegPath::new(Hive::CurrentUser, r"Software\Big");
    for index in 0..=MAX_KEYS {
        system.put(&big.child(&index.to_string()), vec![]);
    }
    let error = watcher.add(big).unwrap_err();
    assert!(
        error.to_string().contains("Watch a key further down"),
        "{error}"
    );
}

#[test]
fn csv_has_a_header_and_one_line_per_event() {
    let event = Event {
        seq: 1,
        time: 5,
        change: Change {
            op: Op::ValueAdded,
            key: RegPath::new(Hive::CurrentUser, "Software"),
            value: Some("a".into()),
            kind: Some("REG_SZ".into()),
            old: None,
            new: Some("x,y".into()),
        },
        actor: Some(Actor {
            pid: 42,
            process: "reg.exe".into(),
        }),
    };
    let text = to_csv(&[event]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].ends_with(",Process,PID"), "{}", lines[0]);
    assert_eq!(
        lines[1],
        r#"5,1970-01-01 00:00:00.005 UTC,ValueAdded,HKCU\Software,a,REG_SZ,,"x,y",reg.exe,42"#
    );
}

#[test]
fn a_change_names_its_process_only_when_tracing_saw_it() {
    let dir = tempfile::tempdir().unwrap();
    let system: Arc<FakeSystem> = Arc::new(FakeSystem::empty(&dir.path().join("machine")));
    let key = RegPath::new(Hive::CurrentUser, r"Software\Acme");
    system.put(&key, vec![]);
    let watcher = Watcher::start(Arc::clone(&system) as Arc<dyn System>, dir.path()).unwrap();
    assert_eq!(watcher.tracing(), TraceState::Idle, "nothing watched");
    watcher.add(key.clone()).unwrap();
    assert_eq!(watcher.tracing(), TraceState::On);
    system
        .write_value(&key, &RawValue::string("Mode", "fast"))
        .unwrap();
    let events = wait_for(&watcher, &key, 1);
    let actor = events[0].actor.clone().expect("the writer is named");
    assert_eq!(actor.pid, std::process::id());
    assert!(!actor.process.is_empty());
    // As a write through a handle opened before tracing started.
    system.put(&key, vec![RawValue::string("Other", "x")]);
    let events = wait_for(&watcher, &key, 2);
    assert_eq!(events[1].change.value.as_deref(), Some("Other"));
    assert_eq!(events[1].actor, None, "unseen writes name no one");
    watcher.pause(&key, true).unwrap();
    assert_eq!(
        watcher.tracing(),
        TraceState::Idle,
        "stopped with the watch"
    );
    watcher.stop_all();
}

#[test]
fn without_administrator_rights_no_process_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let system: Arc<FakeSystem> = Arc::new(FakeSystem::empty(&dir.path().join("machine")));
    system.set_elevated(false);
    let key = RegPath::new(Hive::CurrentUser, r"Software\Acme");
    system.put(&key, vec![]);
    let watcher = Watcher::start(Arc::clone(&system) as Arc<dyn System>, dir.path()).unwrap();
    watcher.add(key.clone()).unwrap();
    assert_eq!(watcher.tracing(), TraceState::NeedsAdmin);
    system
        .write_value(&key, &RawValue::string("Mode", "fast"))
        .unwrap();
    let events = wait_for(&watcher, &key, 1);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].actor, None);
    watcher.stop_all();
}

#[test]
fn a_damaged_watch_list_is_moved_aside_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FILE), "{ broken").unwrap();
    let system: Arc<FakeSystem> = Arc::new(FakeSystem::empty(&dir.path().join("machine")));
    let watcher = Watcher::start(Arc::clone(&system) as Arc<dyn System>, dir.path()).unwrap();
    assert!(watcher.list().is_empty());
    let aside = std::fs::read_to_string(dir.path().join("watch.json.damaged")).unwrap();
    assert_eq!(aside, "{ broken");
}

#[test]
fn a_change_undone_at_once_is_logged_when_tracing_saw_it() {
    let dir = tempfile::tempdir().unwrap();
    let system: Arc<FakeSystem> = Arc::new(FakeSystem::empty(&dir.path().join("machine")));
    let key = RegPath::new(Hive::CurrentUser, r"Software\Acme");
    system.put(&key, vec![RawValue::string("Mode", "slow")]);
    let watcher = Watcher::start(Arc::clone(&system) as Arc<dyn System>, dir.path()).unwrap();
    watcher.add(key.clone()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    // Changed and put back: the readings may never see "fast".
    system
        .write_value(&key, &RawValue::string("Mode", "fast"))
        .unwrap();
    system
        .write_value(&key, &RawValue::string("Mode", "slow"))
        .unwrap();
    let events = wait_for(&watcher, &key, 2);
    let seen: Vec<(Op, Option<&str>, Option<&str>)> = events
        .iter()
        .map(|event| {
            (
                event.change.op,
                event.change.old.as_deref(),
                event.change.new.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            (Op::ValueChanged, Some("slow"), Some("fast")),
            (Op::ValueChanged, Some("fast"), Some("slow")),
        ]
    );
    assert!(events.iter().all(|event| event.actor.is_some()));
    watcher.stop_all();
}
