// Links an ordinary account can make: Linux and macOS.
#![cfg(unix)]

use super::*;
use crate::fake::FakeSystem;

/// A file swapped for a link after the plan was made is refused rather
/// than emptying the link's target, which the guard never saw.
#[test]
fn a_truncate_refuses_a_link_swapped_in_after_planning() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(&dir.path().join("sandbox")).unwrap();
    let target = dir.path().join("elsewhere.log");
    fs::write(&target, b"not this one").unwrap();
    let link = dir.path().join("app.log");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let guard = Guard::default();
    let never = || false;
    let executor = Executor {
        system: &system,
        guard: &guard,
        shred: false,
        delete_on_reboot: false,
        cancelled: &never,
    };
    let outcome = executor.execute(&Op::Truncate {
        path: link,
        size: 12,
    });
    assert!(matches!(outcome, Outcome::Failed { .. }));
    assert_eq!(fs::read(&target).unwrap(), b"not this one");
}
