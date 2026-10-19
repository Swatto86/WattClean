//! Other installed programs: where they live must be known however their
//! entry says it, so that a scan never offers it.

use std::path::Path;

use super::{has, machine, shown, uninstall_and_scan};
use crate::System;
use crate::folders::Folders;
use crate::identity::Identity;
use crate::model::{Hive, RawValue, RegPath};
use crate::programs::{UNINSTALL, list_programs};
use crate::scan::Mode;
use crate::scan::shortcut::link_bytes;

fn hklm(path: &str) -> RegPath {
    RegPath::new(Hive::LocalMachine, path)
}

fn text(name: &str, value: &str) -> RawValue {
    RawValue::string(name, value)
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn widget_dir(folders: &Folders) -> std::path::PathBuf {
    folders.program_files.join(r"Acme\Widget")
}

#[test]
fn an_msi_program_with_no_install_folder_keeps_the_folder_its_files_are_in() {
    let machine = machine();
    let plugin = widget_dir(&machine.folders).join(r"Northwind\nt.dll");
    write(&plugin, b"dll");
    let code = "{11112222-3333-4444-5555-666677778888}";
    let packed = crate::msi::packed_guid(code).unwrap();
    machine.system.put(
        &hklm(&format!(r"{UNINSTALL}\{code}")),
        vec![
            text("DisplayName", "Northwind Tools"),
            text("UninstallString", &format!("MsiExec.exe /X{code}")),
            RawValue::dword("WindowsInstaller", 1),
        ],
    );
    machine.system.put(
        &hklm(crate::msi::COMPONENTS).child("{C9C9C9C9-0000-0000-0000-000000000009}"),
        vec![text(&packed, &plugin.display().to_string())],
    );
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Safe);
    assert!(
        !has(&report, r"Program Files\Acme\Widget"),
        "{:#?}",
        shown(&report)
    );
}

#[test]
fn a_program_known_only_by_its_shortcut_keeps_the_folder_it_points_into() {
    let machine = machine();
    let notes = widget_dir(&machine.folders).join(r"Notes\notes.exe");
    write(&notes, b"exe");
    write(
        &machine
            .folders
            .common_start_menu_programs
            .join("Northwind Notes.lnk"),
        &link_bytes(&notes.display().to_string()),
    );
    machine.system.put(
        &hklm(&format!(r"{UNINSTALL}\NorthwindNotes")),
        vec![
            text("DisplayName", "Northwind Notes"),
            text(
                "UninstallString",
                &machine
                    .folders
                    .windows
                    .join("nwnotes.exe")
                    .display()
                    .to_string(),
            ),
        ],
    );
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Safe);
    assert!(
        !has(&report, r"Program Files\Acme\Widget"),
        "{:#?}",
        shown(&report)
    );
}

#[test]
fn a_folder_another_program_is_installed_in_too_is_left_in_place() {
    let machine = machine();
    let dir = widget_dir(&machine.folders);
    machine.system.put(
        &hklm(&format!(r"{UNINSTALL}\NorthwindKit")),
        vec![
            text("DisplayName", "Northwind Kit"),
            text("InstallLocation", &dir.display().to_string()),
            text(
                "UninstallString",
                &machine
                    .folders
                    .windows
                    .join("nwkit.exe")
                    .display()
                    .to_string(),
            ),
        ],
    );
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Safe);
    assert!(
        !has(&report, r"Program Files\Acme\Widget"),
        "{:#?}",
        shown(&report)
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Northwind Kit is installed in the same folder")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_component_shared_with_another_product_names_no_install_folder() {
    let machine = machine();
    let system: &dyn System = &machine.system;
    let shared = machine
        .folders
        .program_files_x86
        .clone()
        .unwrap()
        .join(r"Common Files\Fabrikam\Shared");
    let dirs = crate::msi::product_dirs(system, crate::fake::VIEWER_PACKED);
    assert!(
        !dirs.iter().any(|dir| crate::paths::same(dir, &shared)),
        "{dirs:?}"
    );
    let viewer = list_programs(system, true)
        .into_iter()
        .find(|program| program.name == "Fabrikam Viewer")
        .unwrap();
    let identity = Identity::of_program(system, &machine.folders, &viewer);
    assert!(
        !identity
            .install_dirs
            .iter()
            .any(|dir| crate::paths::same(dir, &shared)),
        "{:?}",
        identity.install_dirs
    );
}

#[test]
fn a_removal_rechecks_each_leftover_against_the_machine_as_it_is_now() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Safe);
    let folder = report
        .items
        .iter()
        .find(|item| {
            item.target
                .display()
                .ends_with(r"Program Files\Acme\Widget")
        })
        .unwrap()
        .clone();
    let system: &dyn System = &machine.system;
    let identity = Identity::named("Acme Widget", None, vec![widget_dir(&machine.folders)]);
    let recheck = |others: &[crate::scan::OtherProgram]| {
        let context = crate::scan::Context {
            system,
            folders: &machine.folders,
            others,
            progress: &super::Quiet,
            log: None,
            protected: &[],
        };
        crate::scan::still_allowed(&context, &identity, &folder.target)
    };
    let now = || {
        let programs = list_programs(system, true);
        crate::scan::OtherProgram::all(system, &machine.folders, &programs, None)
    };
    assert!(recheck(&now()));
    // Since the scan, another program was installed into that folder.
    let kit = widget_dir(&machine.folders).join("Kit");
    machine.system.put(
        &hklm(&format!(r"{UNINSTALL}\NorthwindKit")),
        vec![
            text("DisplayName", "Northwind Kit"),
            text("InstallLocation", &kit.display().to_string()),
            text(
                "UninstallString",
                &kit.join("uninstall.exe").display().to_string(),
            ),
        ],
    );
    assert!(!recheck(&now()));
}
