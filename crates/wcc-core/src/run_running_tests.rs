//! A selected program that is running: Windows refuses to delete the files
//! it holds open, so only those are skipped; macOS and Linux would delete
//! them from under it, so its options are left alone there.

use std::path::PathBuf;

use super::*;
use crate::catalog::Catalog;
use crate::fake::FakeSystem;
use crate::model::OptionKey;
use crate::report::{Mode, Quiet, Report};
use crate::system::Platform;

/// The test machine (which runs Firefox) posing as `platform`, with the
/// Windows-shaped catalog its files fit.
fn machine(platform: Platform) -> (tempfile::TempDir, FakeSystem, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(dir.path()).unwrap().posing_as(platform);
    let catalog = Catalog::load(Platform::Windows, Some(&dir.path().join("cleaners")), false);
    (dir, system, catalog)
}

fn run_on(system: &FakeSystem, catalog: &Catalog, mode: Mode) -> Report {
    let selection: Vec<OptionKey> = ["firefox.cache", "firefox.cookies", "chrome.cache"]
        .iter()
        .map(|k| OptionKey::parse(k).unwrap())
        .collect();
    run(
        catalog,
        &selection,
        mode,
        system,
        &RunSettings::default(),
        &Quiet,
    )
}

fn firefox_file(system: &FakeSystem) -> PathBuf {
    crate::paths::join(
        &system.drive_c(),
        r"Users\Tester\AppData\Roaming\Mozilla\Firefox\Profiles\t3st.default-release\cookies.sqlite",
    )
}

#[test]
fn on_macos_and_linux_a_running_programs_options_are_left_alone() {
    for (platform, verb) in [(Platform::Macos, "Quit it"), (Platform::Linux, "Close it")] {
        let (_dir, system, catalog) = machine(platform);
        for mode in [Mode::Preview, Mode::Clean] {
            let report = run_on(&system, &catalog, mode);
            assert_eq!(report.running, vec!["Firefox".to_string()]);
            let firefox: Vec<_> = report
                .options
                .iter()
                .filter(|o| o.key.starts_with("firefox."))
                .collect();
            assert_eq!(firefox.len(), 2, "{platform:?}: {:#?}", report.options);
            assert!(
                firefox.iter().all(|o| o.items.is_empty()),
                "{platform:?} {mode:?} planned a running program's files: {firefox:#?}"
            );
            let problems: Vec<_> = firefox.iter().flat_map(|o| &o.problems).collect();
            assert_eq!(problems.len(), 1, "one problem per program: {problems:?}");
            assert_eq!(problems[0].path, "Firefox");
            assert!(problems[0].message.contains(verb), "{:?}", problems[0]);
            let chrome = report.options.iter().find(|o| o.key == "chrome.cache");
            assert!(
                chrome.is_some_and(|o| o.counts.files > 0),
                "a program not running is still cleaned"
            );
        }
        assert!(firefox_file(&system).exists(), "{platform:?} deleted it");
    }
}

#[test]
fn on_windows_a_running_program_loses_what_it_does_not_hold_open() {
    let (_dir, system, catalog) = machine(Platform::Windows);
    let report = run_on(&system, &catalog, Mode::Clean);
    assert_eq!(report.running, vec!["Firefox".to_string()]);
    assert_eq!(report.totals.problems, 0, "{:#?}", report.options);
    assert!(!firefox_file(&system).exists());
}
