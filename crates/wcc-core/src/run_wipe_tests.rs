//! The free-space wipe as a cleaner option, on a test machine whose drive
//! compresses what is written.

use super::*;
use crate::catalog::Catalog;
use crate::fake::FakeSystem;
use crate::model::OptionKey;
use crate::report::{Mode, Quiet};
use crate::system::System;

/// Btrfs mounted with compression stores a wipe's zeros as almost nothing:
/// the option says why it wipes nothing rather than quietly doing nothing.
#[test]
fn free_disk_space_on_a_compressing_drive_is_a_problem_not_a_wipe() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(dir.path()).unwrap().compressing();
    let catalog = Catalog::load(system.platform(), Some(&dir.path().join("cleaners")), false);
    let selection = [OptionKey::parse("system.free_disk_space").unwrap()];
    let report = run(
        &catalog,
        &selection,
        Mode::Preview,
        &system,
        &RunSettings::default(),
        &Quiet,
    );
    let wipe = &report.options[0];
    assert_eq!(wipe.counts.other, 0, "{wipe:#?}");
    assert_eq!(wipe.problems.len(), 1, "{wipe:#?}");
    assert!(wipe.problems[0].message.contains("compresses"), "{wipe:#?}");
}
