//! A package manager's command through a whole run, on the test machine.

use super::*;
use crate::catalog::Catalog;
use crate::fake::FakeSystem;
use crate::model::OptionKey;
use crate::report::{ItemKind, Mode, Quiet};
use crate::system::System;

const RESIDUAL: &str = r#"
[[cleaner]]
id = "apt"
name = "APT"
category = "packages"
detect_program = ["apt-get"]
[[cleaner.option]]
id = "residual"
name = "Leftover settings"
description = "d"
actions = [{ op = "command", task = "apt-residual" }]
[[cleaner.option]]
id = "cache"
name = "Downloaded packages"
description = "d"
actions = [{ op = "command", task = "apt-clean" }]
"#;

fn apt_machine(dpkg_prints: &str) -> (tempfile::TempDir, FakeSystem, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(dir.path()).unwrap();
    std::fs::write(dir.path().join("cleaners").join("apt.toml"), RESIDUAL).unwrap();
    system.add_program("apt-get", "").unwrap();
    system.add_program("dpkg-query", dpkg_prints).unwrap();
    let catalog = Catalog::load(system.platform(), Some(&dir.path().join("cleaners")), false);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    (dir, system, catalog)
}

fn run_apt(catalog: &Catalog, system: &FakeSystem, mode: Mode) -> crate::report::Report {
    let selection = [
        OptionKey::parse("apt.residual").unwrap(),
        OptionKey::parse("apt.cache").unwrap(),
    ];
    run(
        catalog,
        &selection,
        mode,
        system,
        &RunSettings::default(),
        &Quiet,
    )
}

#[test]
fn a_preview_lists_what_a_command_acts_on_and_runs_nothing_else() {
    let (_dir, system, catalog) = apt_machine("ii  bash\nrc  oldapp\nrc  -evil\n");
    let report = run_apt(&catalog, &system, Mode::Preview);
    let residual = &report.options[0].items;
    assert_eq!(residual.len(), 1, "{residual:?}");
    assert_eq!(residual[0].kind, ItemKind::Command);
    assert_eq!(residual[0].path, "apt-get -y purge");
    assert_eq!(
        residual[0].note.as_deref(),
        Some("as administrator: oldapp")
    );
    assert_eq!(report.options[1].items[0].path, "apt-get clean");
    // Only the read-only listing ran.
    assert!(
        system
            .ran()
            .iter()
            .all(|line| line.starts_with("dpkg-query"))
    );
}

#[test]
fn a_clean_runs_the_command_as_administrator_with_the_listed_names() {
    let (_dir, system, catalog) = apt_machine("rc  oldapp\nrc  other:amd64\n");
    let report = run_apt(&catalog, &system, Mode::Clean);
    assert!(
        report.options.iter().all(|o| o.problems.is_empty()),
        "{report:#?}"
    );
    let ran = system.ran();
    assert!(
        ran.contains(&"admin apt-get -y purge oldapp other:amd64".to_string()),
        "{ran:?}"
    );
    assert!(ran.contains(&"admin apt-get clean".to_string()), "{ran:?}");
}

#[test]
fn nothing_listed_means_nothing_to_run() {
    let (_dir, system, catalog) = apt_machine("ii  bash\n");
    let report = run_apt(&catalog, &system, Mode::Clean);
    assert!(report.options[0].items.is_empty());
    assert!(!system.ran().iter().any(|line| line.contains("purge")));
}

#[test]
fn a_missing_package_manager_hides_its_cleaner_and_its_commands() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(dir.path()).unwrap();
    std::fs::write(dir.path().join("cleaners").join("apt.toml"), RESIDUAL).unwrap();
    let catalog = Catalog::load(system.platform(), Some(&dir.path().join("cleaners")), false);
    let apt = catalog.cleaner("apt").unwrap();
    assert!(!crate::catalog::is_present(apt, &system));
    let report = run_apt(&catalog, &system, Mode::Clean);
    assert!(report.options.iter().all(|o| o.items.is_empty()));
    assert!(system.ran().is_empty());
}
