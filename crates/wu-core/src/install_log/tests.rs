use std::path::PathBuf;

use super::*;
use crate::fake::TAILSPIN_SETUP;
use crate::identity::Identity;
use crate::model::Hive;
use crate::programs::list_programs;
use crate::scan::tests::{Machine, Quiet, machine};
use crate::scan::{Context, Mode, OtherProgram, scan};
use crate::system::Launch;

fn traced_install(machine: &Machine) -> InstallLog {
    let launch = Launch {
        program: machine.dir.path().join(TAILSPIN_SETUP),
        args: "/S".into(),
    };
    let mut tracing = machine.system.trace_install(&launch).unwrap();
    while !tracing.finished().unwrap() {}
    let trace = tracing.finish().unwrap();
    build(
        &machine.system,
        &machine.folders,
        &trace,
        "TailspinPaintSetup.exe",
        1_800_000_000,
    )
}

fn shown(log: &InstallLog, recorded: Recorded) -> Vec<String> {
    log.items
        .iter()
        .filter(|item| item.recorded == recorded)
        .map(|item| item.target.display())
        .collect()
}

#[test]
fn a_traced_install_keeps_what_it_left_and_drops_the_noise() {
    let machine = machine();
    let log = traced_install(&machine);
    assert_eq!(log.program.as_deref(), Some("Tailspin Paint"));
    assert!(
        log.program_key
            .as_ref()
            .unwrap()
            .path
            .ends_with(r"Uninstall\TailspinPaint")
    );
    let created = shown(&log, Recorded::Created);
    for ending in [
        r"Program Files\Tailspin",
        r"ProgramData\TSXData",
        r"Public\Desktop\Tailspin Paint.lnk",
        r"Uninstall\TailspinPaint",
        r"HKLM\SOFTWARE\Tailspin",
        r"HKCU\Software\TSX",
        r"CurrentVersion\Run → TSXUpdater",
    ] {
        assert!(
            created.iter().any(|item| item.ends_with(ending)),
            "missing {ending}: {created:#?}"
        );
    }
    for covered in [
        r"Tailspin\Paint\paint.exe",
        r"TSX\Prefs",
        "brushes.db",
        "→ DisplayName",
    ] {
        assert!(
            !created.iter().any(|item| item.contains(covered)),
            "{covered} is inside a created item"
        );
    }
    for noise in ["tsx_setup.tmp", "RestartManager"] {
        assert!(
            !log.items
                .iter()
                .any(|item| item.target.display().contains(noise)),
            "{noise} kept"
        );
    }
    let changed = shown(&log, Recorded::Changed);
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert!(changed[0].ends_with(r"Session Manager\Environment → Path"));
}

#[test]
fn changes_undone_during_the_install_leave_nothing() {
    let key = RegPath::new(Hive::LocalMachine, r"SOFTWARE\Temp\Work");
    let path = PathBuf::from(r"C:\Program Files\App\step.tmp");
    let mut state = State::default();
    for change in [
        Change::FileCreated { path: path.clone() },
        Change::KeyCreated { key: key.clone() },
        Change::ValueSet {
            key: key.clone(),
            name: "A".into(),
            existed: false,
        },
        Change::FileDeleted { path },
        Change::KeyDeleted {
            key: key.parent().unwrap(),
        },
        Change::ValueSet {
            key: RegPath::new(Hive::CurrentUser, "Software\\X"),
            name: "B".into(),
            existed: true,
        },
        Change::ValueSet {
            key: RegPath::new(Hive::CurrentUser, "Software\\X"),
            name: "B".into(),
            existed: true,
        },
    ] {
        state.apply(&change);
    }
    let kept: Vec<LogItem> = state.items.into_iter().flatten().collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].recorded, Recorded::Changed);
}

#[test]
fn logs_are_saved_listed_matched_and_deleted() {
    let machine = machine();
    let dir = machine.dir.path().join("data");
    let mut first = traced_install(&machine);
    save(&dir, &mut first).unwrap();
    let mut second = first.clone();
    save(&dir, &mut second).unwrap();
    assert_ne!(first.id, second.id, "same-second logs get distinct ids");
    assert_eq!(list(&dir).len(), 2);
    let summary = first.summary();
    assert_eq!(summary.changed_items, 1);
    assert!(summary.created_items >= 7);
    let program = list_programs(&machine.system, false)
        .into_iter()
        .find(|p| p.name == "Tailspin Paint")
        .unwrap();
    assert!(for_program(&dir, &program).is_some());
    delete(&dir, &second.id).unwrap();
    assert_eq!(list(&dir).len(), 1);
    for bad in ["..", r"..\x", "a/b", ""] {
        assert!(load(&dir, bad).is_err());
        assert!(delete(&dir, bad).is_err());
    }
}

#[test]
fn the_uninstall_finds_what_only_the_log_knows() {
    let machine = machine();
    let log = traced_install(&machine);
    let system: &dyn System = &machine.system;
    let program = list_programs(system, false)
        .into_iter()
        .find(|p| p.name == "Tailspin Paint")
        .unwrap();
    let identity = Identity::of_program(system, &machine.folders, &program);
    let launch =
        crate::command_line::uninstall_launch(system, &machine.folders, &program, false).unwrap();
    let mut running = system.launch(&launch).unwrap();
    while !running.finished().unwrap() {}
    let others = OtherProgram::all(
        system,
        &machine.folders,
        &list_programs(system, true),
        Some(&program),
    );
    let scan_with = |log| {
        let context = Context {
            system,
            folders: &machine.folders,
            others: &others,
            progress: &Quiet,
            log,
            protected: &[],
        };
        scan(&identity, Mode::Safe, &context)
    };
    let without: Vec<String> = scan_with(None)
        .items
        .iter()
        .map(|i| i.target.display())
        .collect();
    assert!(
        !without.iter().any(|item| item.contains("TSXData")),
        "only the log knows TSXData"
    );
    let report = scan_with(Some(&log));
    let found: Vec<String> = report.items.iter().map(|i| i.target.display()).collect();
    for ending in [r"ProgramData\TSXData", r"HKCU\Software\TSX", "TSXUpdater"] {
        assert!(
            found.iter().any(|item| item.ends_with(ending)),
            "missing {ending}: {found:#?}"
        );
    }
    assert!(
        !found.iter().any(|item| item.contains("Environment")),
        "a changed value is never offered"
    );
    let data = report
        .items
        .iter()
        .find(|i| i.target.display().ends_with("TSXData"))
        .unwrap();
    assert_eq!(data.reason, "Recorded when it was installed");
}

#[test]
fn session_manager_values_are_noise_but_its_environment_is_not() {
    let machine = machine();
    let manager = RegPath::new(
        Hive::LocalMachine,
        r"SYSTEM\CurrentControlSet\Control\Session Manager",
    );
    let pending = Target::Value {
        key: manager.clone(),
        name: "PendingFileRenameOperations".into(),
    };
    let path = Target::Value {
        key: manager.child("Environment"),
        name: "Path".into(),
    };
    assert!(noise::is_noise(&pending, &machine.folders));
    assert!(!noise::is_noise(&path, &machine.folders));
    let restart = Target::Key {
        key: RegPath::new(
            Hive::CurrentUser,
            r"Software\Microsoft\RestartManager\Session0001",
        ),
    };
    assert!(noise::is_noise(&restart, &machine.folders));
    let program = Target::Folder {
        path: machine.folders.program_files.join("Tailspin"),
    };
    assert!(!noise::is_noise(&program, &machine.folders));
}
