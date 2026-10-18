//! What a reading logs: each change with its write, and the writes the
//! readings could not see.

use super::*;
use crate::model::{Hive, REG_DWORD, RegPath};
use crate::trace::Captured;

fn key() -> RegPath {
    RegPath::new(Hive::CurrentUser, r"Software\Acme")
}

/// A reading that began long after the writes and found at `finished`.
fn reading(finished: u64) -> Reading {
    Reading {
        started: finished + 1_000,
        finished,
    }
}

fn dword(value: u32) -> Option<Captured> {
    Some(Captured {
        kind: REG_DWORD,
        size: 4,
        bytes: value.to_le_bytes().to_vec(),
    })
}

fn set(time: u64, pid: u32, new: u32, old: Option<u32>) -> Write {
    Write {
        time,
        actor: Actor {
            pid,
            process: format!("p{pid}.exe"),
        },
        op: WriteOp::ValueSet,
        key: key(),
        value: Some("Size".into()),
        new: dword(new),
        old: old.and_then(dword),
    }
}

fn changed(old: &str, new: &str) -> Change {
    Change {
        op: Op::ValueChanged,
        key: key(),
        value: Some("Size".into()),
        kind: Some("REG_DWORD".into()),
        old: Some(old.into()),
        new: Some(new.into()),
    }
}

/// Time, operation, old and new data, process id.
type Row = (u64, Op, Option<String>, Option<String>, Option<u32>);

fn shown(found: &[Found]) -> Vec<Row> {
    found
        .iter()
        .map(|entry| {
            (
                entry.time,
                entry.change.op,
                entry.change.old.clone(),
                entry.change.new.clone(),
                entry.actor.as_ref().map(|actor| actor.pid),
            )
        })
        .collect()
}

#[test]
fn one_write_gives_the_change_its_process_and_time() {
    let writes = [set(10, 7, 2, Some(1))];
    let Explained { found, .. } = explain(
        vec![changed("1", "2")],
        &writes,
        &HashSet::new(),
        reading(99),
    );
    assert_eq!(
        shown(&found),
        vec![(
            10,
            Op::ValueChanged,
            Some("1".into()),
            Some("2".into()),
            Some(7)
        )]
    );
}

#[test]
fn without_tracing_the_change_is_logged_when_found() {
    let Explained { found, .. } =
        explain(vec![changed("1", "2")], &[], &HashSet::new(), reading(99));
    assert_eq!(
        shown(&found),
        vec![(
            99,
            Op::ValueChanged,
            Some("1".into()),
            Some("2".into()),
            None
        )]
    );
}

#[test]
fn several_writes_between_readings_are_each_logged() {
    let writes = [set(10, 7, 2, Some(1)), set(20, 8, 3, Some(2))];
    let Explained { found, .. } = explain(
        vec![changed("1 (whole)", "3 (whole)")],
        &writes,
        &HashSet::new(),
        reading(99),
    );
    let two = format!("0x{:08x} (2)", 2);
    assert_eq!(
        shown(&found),
        vec![
            (
                10,
                Op::ValueChanged,
                Some("1 (whole)".into()),
                Some(two.clone()),
                Some(7)
            ),
            (
                20,
                Op::ValueChanged,
                Some(two),
                Some("3 (whole)".into()),
                Some(8)
            ),
        ]
    );
}

#[test]
fn a_change_undone_before_the_next_reading_is_still_logged() {
    let created = Write {
        op: WriteOp::KeyCreated,
        new: None,
        old: None,
        value: None,
        ..set(5, 3, 0, None)
    };
    let deleted = Write {
        op: WriteOp::KeyDeleted,
        time: 6,
        ..created.clone()
    };
    let writes = [
        created,
        set(10, 7, 2, Some(1)),
        set(20, 7, 1, Some(2)),
        deleted,
        // Rewritten with the same data: nothing changed.
        set(30, 9, 1, Some(1)),
    ];
    let Explained { found, logged, .. } =
        explain(Vec::new(), &writes, &HashSet::new(), reading(99));
    let ops: Vec<(u64, Op)> = found
        .iter()
        .map(|entry| (entry.time, entry.change.op))
        .collect();
    assert_eq!(
        ops,
        vec![
            (5, Op::KeyCreated),
            (6, Op::KeyDeleted),
            (10, Op::ValueChanged),
            (20, Op::ValueChanged),
        ]
    );
    assert_eq!(found[2].change.new.as_deref(), Some("0x00000002 (2)"));
    // The next window overlaps this one: nothing is logged twice.
    let Explained { found: again, .. } = explain(Vec::new(), &writes, &logged, reading(199));
    assert!(again.is_empty(), "{again:?}");
}

#[test]
fn data_seen_only_in_part_says_so() {
    let mut write = set(1, 1, 0, None);
    write.new = Some(Captured {
        kind: crate::model::REG_BINARY,
        size: 1000,
        bytes: vec![0xab; 4],
    });
    let Explained { found, .. } = explain(Vec::new(), &[write], &HashSet::new(), reading(9));
    let new = found[0].change.new.clone().unwrap();
    assert!(new.ends_with("(first 4 of 1000 bytes)"), "{new}");
    assert_eq!(found[0].change.op, Op::ValueAdded);
}

#[test]
fn a_write_during_a_reading_waits_for_the_next_one() {
    let write = set(1_050, 7, 2, Some(1));
    let first = Reading {
        started: 1_000,
        finished: 1_100,
    };
    let Explained {
        found,
        logged,
        waiting,
    } = explain(
        Vec::new(),
        std::slice::from_ref(&write),
        &HashSet::new(),
        first,
    );
    assert!(found.is_empty(), "logged as never seen: {found:?}");
    assert!(waiting, "another reading should follow");
    let second = Reading {
        started: 1_300,
        finished: 1_400,
    };
    let Explained { found, .. } = explain(vec![changed("1", "2")], &[write], &logged, second);
    assert_eq!(
        shown(&found),
        vec![(
            1_050,
            Op::ValueChanged,
            Some("1".into()),
            Some("2".into()),
            Some(7)
        )],
        "the next reading's change gets its program"
    );
}

#[test]
fn a_write_without_data_is_a_value_set() {
    let mut write = set(5, 3, 1, None);
    write.new = None;
    let Explained { found, .. } = explain(Vec::new(), &[write], &HashSet::new(), reading(99));
    assert_eq!(found[0].change.op, Op::ValueSet);
    assert_eq!(found[0].change.new, None);
}

#[test]
fn a_write_after_a_reading_ends_belongs_to_the_next_reading() {
    // The first reading saw the value added as 2; the second write (to
    // 1) came just after it ended, and the next reading found that.
    let writes = [set(990, 1, 2, None), set(1_040, 2, 1, Some(2))];
    let first = Reading {
        started: 1_000,
        finished: 1_010,
    };
    let mut added = changed("", "2");
    added.op = Op::ValueAdded;
    added.old = None;
    let one = explain(vec![added], &writes, &HashSet::new(), first);
    assert_eq!(
        shown(&one.found),
        vec![(990, Op::ValueAdded, None, Some("2".into()), Some(1))]
    );
    assert!(one.waiting, "the later write waits");
    let second = Reading {
        started: 1_050,
        finished: 1_060,
    };
    let two = explain(vec![changed("2", "1")], &writes, &one.logged, second);
    assert_eq!(
        shown(&two.found),
        vec![(
            1_040,
            Op::ValueChanged,
            Some("2".into()),
            Some("1".into()),
            Some(2)
        )]
    );
}
