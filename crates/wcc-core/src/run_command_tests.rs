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

const HOMEBREW: &str = r#"
[[cleaner]]
id = "homebrew"
name = "Homebrew"
category = "packages"
detect_program = ["brew"]
[[cleaner.option]]
id = "cleanup"
name = "Downloads and old versions"
description = "d"
actions = [{ op = "command", task = "brew-cleanup" }]
[[cleaner.option]]
id = "autoremove"
name = "Unneeded dependencies"
description = "d"
actions = [{ op = "command", task = "brew-autoremove" }]
[[cleaner]]
id = "macports"
name = "MacPorts"
category = "packages"
detect_program = ["port"]
[[cleaner.option]]
id = "clean"
name = "Build files and downloads"
description = "d"
actions = [{ op = "command", task = "port-clean" }]
"#;

fn homebrew_machine() -> (tempfile::TempDir, FakeSystem, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::open(dir.path()).unwrap();
    std::fs::write(dir.path().join("cleaners").join("brew.toml"), HOMEBREW).unwrap();
    let catalog = Catalog::load(system.platform(), Some(&dir.path().join("cleaners")), false);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    (dir, system, catalog)
}

fn run_keys(
    catalog: &Catalog,
    system: &FakeSystem,
    keys: &[&str],
    mode: Mode,
) -> crate::report::Report {
    let selection: Vec<OptionKey> = keys.iter().map(|k| OptionKey::parse(k).unwrap()).collect();
    run(
        catalog,
        &selection,
        mode,
        system,
        &RunSettings::default(),
        &Quiet,
    )
}

/// `brew cleanup` also uninstalls the formulae nothing needs unless told
/// not to: that is the separate, warned "Unneeded dependencies" option.
#[test]
fn homebrews_cleanup_never_uninstalls_formulae() {
    let (_dir, system, catalog) = homebrew_machine();
    system.add_program("brew", "").unwrap();
    let report = run_keys(&catalog, &system, &["homebrew.cleanup"], Mode::Clean);
    assert!(report.options[0].problems.is_empty(), "{report:#?}");
    let ran = system.ran();
    assert_eq!(
        ran,
        [
            "HOMEBREW_NO_AUTOREMOVE=1 brew cleanup --prune=all -n",
            "HOMEBREW_NO_AUTOREMOVE=1 brew cleanup --prune=all",
        ]
    );
}

/// The autoremove option's preview is its safety measure: a formula from
/// a tap (`user/repo/name`) is listed as Homebrew prints it, and a name
/// that cannot be shown is still counted.
#[test]
fn the_autoremove_preview_names_every_formula_it_removes() {
    let (_dir, system, catalog) = homebrew_machine();
    system
        .add_program(
            "brew",
            "==> Would autoremove 3 unneeded formulae:\nhashicorp/tap/foo\nlibyaml\n$(id)\n",
        )
        .unwrap();
    let report = run_keys(&catalog, &system, &["homebrew.autoremove"], Mode::Preview);
    let items = &report.options[0].items;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(
        items[0].note.as_deref(),
        Some("1 name that cannot be shown, hashicorp/tap/foo, libyaml")
    );
}

/// A program run as administrator never comes from a folder the account
/// (or anything it runs) can write: a `port` planted in Homebrew's folder
/// is neither run nor offered.
#[test]
fn an_administrator_command_never_runs_a_program_the_account_could_have_planted() {
    let (_dir, system, catalog) = homebrew_machine();
    system.add_untrusted_program("port", "").unwrap();
    for mode in [Mode::Preview, Mode::Clean] {
        let report = run_keys(&catalog, &system, &["macports.clean"], mode);
        let option = &report.options[0];
        assert!(
            option.items.is_empty() && option.problems.is_empty(),
            "{report:#?}"
        );
    }
    assert!(system.ran().is_empty(), "{:?}", system.ran());

    let (_dir, system, catalog) = homebrew_machine();
    system.add_program("port", "").unwrap();
    run_keys(&catalog, &system, &["macports.clean"], Mode::Clean);
    assert_eq!(system.ran(), ["admin port clean --all installed"]);
}
