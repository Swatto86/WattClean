use super::*;
use crate::fake::FakeSystem;
use crate::programs::{find_program, list_programs};

mod guard;
mod hardening;
mod others;
mod registrations;

pub struct Quiet;

impl Progress for Quiet {
    fn area(&self, _area: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

pub struct Machine {
    pub dir: tempfile::TempDir,
    pub system: FakeSystem,
    pub folders: Folders,
}

pub fn machine() -> Machine {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(dir.path()).unwrap();
    let folders = Folders::load(&system).unwrap();
    Machine {
        dir,
        system,
        folders,
    }
}

/// Uninstall `name` with its (fake) uninstaller, then scan in `mode`.
pub fn uninstall_and_scan(machine: &Machine, name: &str, mode: Mode) -> ScanReport {
    let system: &dyn System = &machine.system;
    let program = list_programs(system, true)
        .into_iter()
        .find(|p| p.name == name)
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
    let context = Context {
        system,
        folders: &machine.folders,
        others: &others,
        progress: &Quiet,
        log: None,
        protected: &[],
    };
    scan(&identity, mode, &context)
}

pub(super) fn shown(report: &ScanReport) -> Vec<String> {
    report
        .items
        .iter()
        .map(|item| item.target.display())
        .collect()
}

pub(super) fn has(report: &ScanReport, ending: &str) -> bool {
    shown(report).iter().any(|item| item.ends_with(ending))
}

#[test]
fn safe_mode_finds_the_programs_own_folders_and_keys() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Safe);
    let want = [
        r"HKLM\SOFTWARE\Acme\Widget",
        r"HKCU\Software\Acme\Widget",
        r"Program Files\Acme\Widget",
        r"Roaming\Acme\Widget",
        r"Local\Acme Widget",
        r"ProgramData\Acme\Widget",
        r"Start Menu\Programs\Acme Widget",
        r"Public\Desktop\Acme Widget.lnk",
    ];
    for ending in want {
        assert!(
            has(&report, ending),
            "missing {ending} in {:#?}",
            shown(&report)
        );
    }
    let never = [
        r"HKLM\SOFTWARE\Acme",
        r"Program Files\Acme",
        r"ProgramData\Acme",
        r"Acme Widget Helper",
        r"Acme\Helper",
        "notes.txt",
        "App Paths\\widget.exe",
    ];
    for ending in never {
        assert!(
            !has(&report, ending),
            "{ending} must not be offered: {:#?}",
            shown(&report)
        );
    }
    assert!(report.items.iter().all(|item| item.selected));
    let folder = report
        .items
        .iter()
        .find(|item| {
            item.target
                .display()
                .ends_with(r"Program Files\Acme\Widget")
        })
        .unwrap();
    assert!(
        folder.bytes > 2000,
        "folder size is counted: {}",
        folder.bytes
    );
    assert!(
        !has(&report, "settings.ini"),
        "files inside a found folder are not listed twice"
    );
}

#[test]
fn moderate_mode_follows_the_install_path_through_windows_registrations() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Moderate);
    let want = [
        r"App Paths\widget.exe",
        r"CurrentVersion\Run → AcmeWidget",
        r"Plugins\core.dll",
        r"CLSID\{11111111-2222-3333-4444-555555555555}",
        r"Classes\AcmeWidget.Document",
        r"Services\AcmeWidgetSvc",
        r"FirewallRules → {AAAAAAAA-0000-0000-0000-000000000001}",
        r"widget.exe.FriendlyAppName",
        r"ContextMenuHandlers\AcmeWidget",
        r"Applications\widget.exe",
        r"Approved → {11111111-2222-3333-4444-555555555555}",
    ];
    let lower: Vec<String> = shown(&report)
        .iter()
        .map(|item| item.to_lowercase())
        .collect();
    for ending in want {
        assert!(
            lower
                .iter()
                .any(|item| item.ends_with(&ending.to_lowercase())),
            "missing {ending} in {:#?}",
            shown(&report)
        );
    }
    // The shared .wdg key stays; only its entries naming the program go.
    assert!(!has(&report, r"Classes\.wdg"), "{:#?}", shown(&report));
    for absent in [
        "OtherTool",
        "{99999999",
        "Acme Widget Helper",
        r"Applications\uninstall.exe",
    ] {
        assert!(
            !shown(&report).iter().any(|item| item.contains(absent)),
            "{absent} offered"
        );
    }
}

#[test]
fn a_helper_executables_folder_is_only_a_possible_leftover() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Moderate);
    let helper = report
        .items
        .iter()
        .find(|item| item.target.display().ends_with(r"Local\widget-svc"))
        .unwrap_or_else(|| panic!("{:#?}", shown(&report)));
    assert_eq!(helper.confidence, Confidence::Low);
    assert!(!helper.selected);
}

#[test]
fn advanced_mode_adds_low_confidence_finds_unticked() {
    let machine = machine();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Advanced);
    let log = report
        .items
        .iter()
        .find(|item| item.target.display().ends_with("AcmeWidget_install.log"))
        .unwrap_or_else(|| panic!("{:#?}", shown(&report)));
    assert_eq!(log.confidence, Confidence::Low);
    assert!(!log.selected);
    assert!(!has(&report, r"Acme Widget Helper"));
}

#[test]
fn per_user_and_msi_programs_leave_their_own_traces() {
    let machine = machine();
    let notes = uninstall_and_scan(&machine, "Contoso Notes", Mode::Safe);
    for ending in [
        r"HKCU\Software\ContosoNotes",
        r"Roaming\Contoso Notes",
        r"Programs\Contoso Notes.lnk",
    ] {
        assert!(
            has(&notes, ending),
            "missing {ending} in {:#?}",
            shown(&notes)
        );
    }

    let viewer = uninstall_and_scan(&machine, "Fabrikam Viewer", Mode::Safe);
    let products = format!(r"S-1-5-18\Products\{}", crate::fake::VIEWER_PACKED);
    for ending in [
        products.as_str(),
        r"HKCU\Software\Fabrikam\Viewer",
        r"Roaming\Fabrikam\Viewer",
    ] {
        assert!(
            has(&viewer, ending),
            "missing {ending} in {:#?}",
            shown(&viewer)
        );
    }
    assert!(
        !has(&viewer, r"HKCU\Software\Fabrikam"),
        "the publisher also makes Fabrikam Runtime Core"
    );
}

#[test]
fn a_folder_shared_with_another_program_is_kept_and_reported() {
    let machine = machine();
    let system: &dyn System = &machine.system;
    let programs = list_programs(system, true);
    let widget = programs
        .iter()
        .find(|p| p.name.starts_with("Acme Widget 2"))
        .unwrap();
    let mut identity = Identity::of_program(system, &machine.folders, widget);
    let helper_dir = machine.folders.program_files.join(r"Acme\Helper");
    identity.install_dirs.push(helper_dir.clone());
    let others = OtherProgram::all(system, &machine.folders, &programs, Some(widget));
    let context = Context {
        system,
        folders: &machine.folders,
        others: &others,
        progress: &Quiet,
        log: None,
        protected: &[],
    };
    let report = scan(&identity, Mode::Safe, &context);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("Acme Widget Helper"))
    );
    assert!(!has(&report, r"Acme\Helper"));
}

#[test]
fn a_shortcut_into_the_install_folder_is_found_whatever_its_name() {
    let machine = machine();
    let target = machine
        .folders
        .program_files
        .join(r"Acme\Widget\widget.exe");
    let link = machine.folders.desktop.join("Gadget Studio.lnk");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::fs::write(
        &link,
        shortcut::tests::link_to(&target.display().to_string()),
    )
    .unwrap();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Safe);
    let item = report
        .items
        .iter()
        .find(|item| item.target.display().ends_with("Gadget Studio.lnk"))
        .unwrap_or_else(|| panic!("{:#?}", shown(&report)));
    assert_eq!(item.confidence, Confidence::High);
}

#[test]
fn programs_without_folders_still_scan_by_name() {
    let machine = machine();
    let system: &dyn System = &machine.system;
    assert!(
        find_program(
            system,
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\BrokenTool"
        )
        .is_some()
    );
    let identity = Identity::named(
        "Northwind Broken Tool",
        Some("Northwind Traders"),
        Vec::new(),
    );
    // A forced uninstall of a still-listed program leaves that entry out of
    // the programs to protect, or its own keys would count as another's.
    let programs = list_programs(system, true);
    let broken = programs
        .iter()
        .find(|p| p.name == "Northwind Broken Tool")
        .unwrap();
    let others = OtherProgram::all(system, &machine.folders, &programs, Some(broken));
    let context = Context {
        system,
        folders: &machine.folders,
        others: &others,
        progress: &Quiet,
        log: None,
        protected: &[],
    };
    let report = scan(&identity, Mode::Moderate, &context);
    assert!(
        has(&report, r"HKCU\Software\Northwind") && has(&report, r"Program Files\Northwind"),
        "{:#?}",
        shown(&report)
    );
}
