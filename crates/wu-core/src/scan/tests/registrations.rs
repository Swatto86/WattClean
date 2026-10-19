//! The deeper registrations: scheduled tasks, event logs, fonts, startup
//! approvals, default programs, open-with lists, COM interfaces, Windows
//! Installer component records, other accounts, empty publisher shells,
//! nested Start Menu folders and duplicate uninstall entries.

use super::{Quiet, has, machine, shown, uninstall_and_scan};
use crate::fake::{
    APP_ID, BUNDLE_ID, BUNDLE_PACKAGE, INTERFACE_ID, TASK_ID, VIEWER_CODE, VIEWER_PACKED,
};
use crate::identity::Identity;
use crate::model::{Hive, RawValue, RegPath};
use crate::programs::list_programs;
use crate::scan::{Confidence, Context, Mode, OtherProgram, scan};
use crate::system::System;

#[test]
fn moderate_mode_finds_windows_deeper_records_of_its_files() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Moderate);
    let want = [
        format!(r"TaskCache\Tasks\{TASK_ID}"),
        r"TaskCache\Tree\Acme\Widget Update".into(),
        format!(r"TaskCache\Logon\{TASK_ID}"),
        r"System32\Tasks\Acme\Widget Update".into(),
        r"EventLog\Application\AcmeWidget".into(),
        r"Fonts → Acme Sans (TrueType)".into(),
        r"StartupApproved\Run → AcmeWidget".into(),
        r"Clients\Media\AcmeWidget".into(),
        r"RegisteredApplications → Acme Widget".into(),
        r"Classes\.wdg\OpenWithProgids → AcmeWidget.Document".into(),
        r"Classes\.wdg → (Default)".into(),
        r"FileExts\.wdg\OpenWithProgids → AcmeWidget.Document".into(),
        r"FileExts\.wdg\OpenWithList → a".into(),
        r"Desktop\Help and support.url".into(),
        r"Programs\Acme\Tools\Widget Tools.lnk".into(),
        r"ARPCache\AcmeWidget".into(),
    ];
    for ending in &want {
        assert!(
            has(&report, ending),
            "missing {ending} in {:#?}",
            shown(&report)
        );
    }
    let never = [
        "WinSAT",
        "Windows Error Reporting",
        "Arial",
        "OtherTool",
        r"Clients\Media\WMP",
        "Windows Media Player",
        "OpenWithList → b",
        "Bank.url",
        INTERFACE_ID,
        APP_ID,
        r"Users\other",
    ];
    for fragment in never {
        assert!(
            !shown(&report).iter().any(|item| item.contains(fragment)),
            "{fragment} offered: {:#?}",
            shown(&report)
        );
    }
    let default = report
        .items
        .iter()
        .find(|item| item.target.display().ends_with(r"Classes\.wdg → (Default)"))
        .unwrap();
    assert_eq!(default.confidence, Confidence::Medium);
}

#[test]
fn advanced_mode_adds_com_interfaces_and_other_accounts() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Advanced);
    for ending in [
        format!(r"Classes\Interface\{INTERFACE_ID}"),
        format!(r"Classes\AppID\{APP_ID}"),
        r"Users\other\AppData\Roaming\Acme Widget".into(),
    ] {
        assert!(
            has(&report, &ending),
            "missing {ending} in {:#?}",
            shown(&report)
        );
    }
    for fragment in ["{00020400-", r"other\Documents"] {
        assert!(
            !shown(&report).iter().any(|item| item.contains(fragment)),
            "{fragment} offered"
        );
    }
}

#[test]
fn msi_products_leave_component_and_upgrade_records() {
    let machine = machine();
    let system: &dyn System = &machine.system;
    let viewer = list_programs(system, false)
        .into_iter()
        .find(|p| p.name == "Fabrikam Viewer")
        .unwrap();
    let identity = Identity::of_program(system, &machine.folders, &viewer);
    let shared = machine
        .folders
        .program_files_x86
        .clone()
        .unwrap()
        .join(r"Common Files\Fabrikam\Shared");
    assert!(
        !identity
            .install_dirs
            .iter()
            .any(|dir| crate::paths::same(dir, &shared)),
        "a component shared with another product names no install folder: {:?}",
        identity.install_dirs
    );
    let report = uninstall_and_scan(&machine, "Fabrikam Viewer", Mode::Moderate);
    for ending in [
        format!(r"Components\{{C1C1C1C1-0000-0000-0000-000000000001}} → {VIEWER_PACKED}"),
        format!(r"Components\{{C2C2C2C2-0000-0000-0000-000000000002}} → {VIEWER_PACKED}"),
        format!(r"UpgradeCodes\{{U1U1U1U1-0000-0000-0000-000000000001}} → {VIEWER_PACKED}"),
        format!(r"ARPCache\{VIEWER_CODE}"),
    ] {
        assert!(
            has(&report, &ending),
            "missing {ending} in {:#?}",
            shown(&report)
        );
    }
    assert!(
        !shown(&report)
            .iter()
            .any(|item| item.contains("FFFFFFFF0000")
                || item.ends_with("{C3C3C3C3-0000-0000-0000-000000000003}")
                || item.ends_with(r"Common Files\Fabrikam\Shared")),
        "another product's records stay: {:#?}",
        shown(&report)
    );
}

#[test]
fn an_uninstaller_that_empties_the_publisher_leaves_shells_the_scan_finds() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Contoso Notes", Mode::Moderate);
    for ending in [
        r"HKCU\Software\Contoso",
        r"AppData\Local\Contoso",
        r"Start Menu\Programs\Contoso",
        r"Services\ContosoNotes",
    ] {
        assert!(
            has(&report, ending),
            "missing {ending} in {:#?}",
            shown(&report)
        );
    }
    let reason = |ending: &str| {
        report
            .items
            .iter()
            .find(|item| item.target.display().ends_with(ending))
            .map(|item| (item.reason.clone(), item.confidence))
            .unwrap()
    };
    assert_eq!(
        reason(r"HKCU\Software\Contoso"),
        ("Empty publisher key".into(), Confidence::Medium)
    );
    assert_eq!(
        reason(r"AppData\Local\Contoso"),
        ("Empty publisher folder".into(), Confidence::Medium)
    );
    assert_eq!(
        reason(r"Programs\Contoso").0,
        "Start menu folder holding only the program's shortcuts"
    );
    assert_eq!(reason(r"Services\ContosoNotes").1, Confidence::Medium);
    // The nested shortcut is inside the offered folder, so it is not listed twice.
    assert!(!has(&report, "Notes Tools.lnk"));
}

#[test]
fn another_entry_of_the_same_program_is_a_leftover_not_a_neighbour() {
    let machine = machine();
    let duplicate = RegPath::new(
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\AcmeWidgetUser",
    );
    machine.system.put(
        &duplicate,
        vec![
            RawValue::string("DisplayName", "Acme Widget 2.1 (x64)"),
            RawValue::string("UninstallString", r"C:\nowhere\uninstall.exe"),
        ],
    );
    let system: &dyn System = &machine.system;
    let programs = list_programs(system, true);
    assert_eq!(
        programs
            .iter()
            .filter(|p| p.name.starts_with("Acme Widget 2"))
            .count(),
        2
    );
    let widget = programs
        .iter()
        .find(|p| p.registry_key.hive == Hive::LocalMachine && p.name.starts_with("Acme Widget 2"))
        .unwrap();
    let others = OtherProgram::all(system, &machine.folders, &programs, Some(widget));
    assert!(
        !others
            .iter()
            .any(|other| other.key.folded() == duplicate.folded())
    );
    let identity = Identity::of_program(system, &machine.folders, widget);
    let context = Context {
        system,
        folders: &machine.folders,
        others: &others,
        progress: &Quiet,
        log: None,
        protected: &[],
    };
    let report = scan(&identity, Mode::Safe, &context);
    let entry = report
        .items
        .iter()
        .find(|item| item.target.display().ends_with(r"Uninstall\AcmeWidgetUser"))
        .unwrap_or_else(|| panic!("{:#?}", shown(&report)));
    assert_eq!(entry.reason, "Another entry of the same program");
}

#[test]
fn advanced_mode_reads_another_accounts_registry_and_keeps_its_own_programs() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Advanced);
    for ending in [
        r"HKU\Account-other\Software\Acme\Widget",
        r"HKU\Account-other\Software\Microsoft\Windows\CurrentVersion\Run → AcmeWidget",
        r"HKU\Account-other\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\AcmeWidgetUser",
        r"HKU\Account-other_Classes\CLSID\{11111111-2222-3333-4444-555555555555}",
        r"HKU\Account-other_Classes\AcmeWidget.Document",
    ] {
        assert!(
            has(&report, ending),
            "missing {ending} in {:#?}",
            shown(&report)
        );
    }
    for fragment in ["OtherUserTool", r"Account-other\Software\Acme → "] {
        assert!(
            !shown(&report).iter().any(|item| item.contains(fragment)),
            "{fragment} offered: {:#?}",
            shown(&report)
        );
    }
    assert!(
        machine.system.hive_releases() == 0,
        "the engine does not unload hives; the app does after each phase"
    );
    let fresh = super::machine();
    let moderate = uninstall_and_scan(&fresh, "Acme Widget 2.1 (x64)", Mode::Moderate);
    assert!(!shown(&moderate).iter().any(|item| item.starts_with("HKU")));
}

#[test]
fn a_bundle_leaves_its_cache_and_dependency_records() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Woodgrove Runtime", Mode::Safe);
    let want = [
        format!(r"Package Cache\{BUNDLE_ID}"),
        r"Installer\Dependencies\WoodgroveRuntime".into(),
        format!(r"Installer\Dependencies\{BUNDLE_PACKAGE}\Dependents\{BUNDLE_ID}"),
        format!(r"Package Cache\{BUNDLE_PACKAGE}v1.0.0"),
    ];
    for ending in &want {
        assert!(
            has(&report, ending),
            "missing {ending} in {:#?}",
            shown(&report)
        );
    }
    assert!(
        !shown(&report).iter().any(|item| item.contains("C0C0C0C0")),
        "another bundle's cache and dependency stay: {:#?}",
        shown(&report)
    );
    let package = report
        .items
        .iter()
        .find(|item| item.target.display().ends_with("v1.0.0"))
        .unwrap();
    assert_eq!(package.confidence, Confidence::Medium);
    assert!(
        !has(&report, r"Package Cache"),
        "Package Cache itself is never offered"
    );
}
