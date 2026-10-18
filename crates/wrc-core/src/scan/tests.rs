//! The scan modes on the seeded test machine: every broken entry is found in
//! the mode that should find it, and nothing healthy ever is.

use super::*;
use crate::fake::{FakeSystem, paths};

fn machine() -> (tempfile::TempDir, FakeSystem) {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(dir.path()).unwrap();
    (dir, system)
}

fn defaults() -> Options {
    Options {
        exclude_removable: true,
        exclude_windows: true,
        ..Options::default()
    }
}

fn run(system: &FakeSystem, mode: Mode, all_accounts: bool, options: &Options) -> Report {
    let request = Request {
        mode,
        categories: Vec::new(),
        all_accounts,
    };
    scan(system, &request, options, &Progress::default())
}

/// `HKCU\Key` or `HKCU\Key | Value`, with the sandbox folder as `<root>`.
fn shown(dir: &tempfile::TempDir, report: &Report) -> Vec<String> {
    let root = dir.path().to_string_lossy().into_owned();
    report
        .findings
        .iter()
        .map(|finding| {
            let text = match &finding.value {
                None => finding.key.display(),
                Some(value) => format!("{} | {value}", finding.key.display()),
            };
            text.replace(&root, "<root>")
        })
        .collect()
}

const SAFE: [&str; 8] = [
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run | FabrikamUpdater",
    r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run | FabrikamAgent",
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run | GhostApp",
    r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\fabupdate.exe",
    r"HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache | <root>\Program Files\Fabrikam\Updater\update.exe.FriendlyAppName",
    r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\SharedDLLs | <root>\Program Files\Fabrikam\Shared\fabcore.dll",
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\FabrikamUpdater",
    r"HKCU\Software\Classes\Directory\shell\FabrikamScan",
];

#[test]
fn safe_mode_finds_each_broken_entry_and_nothing_healthy() {
    let (dir, system) = machine();
    let report = run(&system, Mode::Safe, false, &defaults());
    let mut found = shown(&dir, &report);
    found.sort();
    let mut want: Vec<String> = SAFE.iter().map(|text| (*text).to_owned()).collect();
    want.sort();
    assert_eq!(found, want);
    assert!(report.findings.iter().all(|finding| finding.ticked));
    let ids: Vec<u32> = report.findings.iter().map(|finding| finding.id).collect();
    assert_eq!(ids, (1..=8).collect::<Vec<_>>());
    assert!(report.notes.is_empty(), "{:?}", report.notes);
}

#[test]
fn moderate_and_advanced_add_their_categories() {
    let (dir, system) = machine();
    let moderate = shown(&dir, &run(&system, Mode::Moderate, false, &defaults()));
    for want in [
        r"HKLM\SOFTWARE\Classes\CLSID\{F00DFACE-0000-4000-8000-000000000001}",
        r"HKLM\SOFTWARE\Classes\CLSID\{F00DFACE-0000-4000-8000-000000000004}",
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\Folders | <root>\Program Files\Fabrikam\",
        r"HKLM\SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules | {F00DFACE-0000-4000-8000-0000000000FF}",
        r"HKCU\AppEvents\Schemes\Apps\Fabrikam\Alert\.Current | ",
        r"HKCU\Software\Classes\.fabdoc",
    ] {
        assert!(
            moderate.iter().any(|found| found == want),
            "missing {want}\n{moderate:#?}"
        );
    }
    assert_eq!(moderate.len(), SAFE.len() + 6, "{moderate:#?}");

    let report = run(&system, Mode::Advanced, false, &defaults());
    let advanced = shown(&dir, &report);
    for want in [
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\SharedDLLs | <root>\Program Files\Contoso\Notes\spell.dll",
        r"HKLM\SYSTEM\CurrentControlSet\Services\FabrikamSvc",
        r"HKCU\Software\Northwind",
        r"HKCU\Software\Classes\Fabrikam.Report",
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
    ] {
        assert!(
            advanced.iter().any(|found| found == want),
            "missing {want}\n{advanced:#?}"
        );
    }
    assert_eq!(advanced.len(), SAFE.len() + 11, "{advanced:#?}");
    let unticked: Vec<Category> = report
        .findings
        .iter()
        .filter(|finding| !finding.ticked)
        .map(|finding| finding.category)
        .collect();
    assert_eq!(
        unticked,
        vec![
            Category::UnusedSharedDlls,
            Category::Services,
            Category::EmptyKeys
        ]
    );
}

#[test]
fn healthy_entries_are_never_offered() {
    let (dir, system) = machine();
    let advanced = shown(&dir, &run(&system, Mode::Advanced, true, &defaults()));
    for healthy in [
        "ContosoNotes",
        "notes.exe",
        "notes.dll",
        "C0FFEE00",
        r"\.txt",
        "txtfile",
    ] {
        assert!(
            advanced.iter().all(|found| !found.contains(healthy)),
            "{healthy} was offered: {advanced:#?}"
        );
    }
}

#[test]
fn unplugged_drives_and_windows_paths_are_left_unless_asked() {
    let (dir, system) = machine();
    let safe = shown(&dir, &run(&system, Mode::Safe, false, &defaults()));
    assert!(
        safe.iter()
            .all(|found| !found.contains("PortableTool") && !found.contains("OldWindowsHelper"))
    );
    let options = Options {
        exclude_removable: false,
        exclude_windows: false,
        ..Options::default()
    };
    let wider = shown(&dir, &run(&system, Mode::Safe, false, &options));
    assert!(
        wider.iter().any(|found| found.ends_with("| PortableTool")),
        "{wider:#?}"
    );
    assert!(
        wider
            .iter()
            .any(|found| found.ends_with("| OldWindowsHelper")),
        "{wider:#?}"
    );
}

#[test]
fn other_accounts_are_scanned_on_request_and_closed_after() {
    let (dir, system) = machine();
    let before = system.hive_releases();
    let report = run(&system, Mode::Safe, true, &defaults());
    let found = shown(&dir, &report);
    let bob = r"HKU\Account-Bob\Software\Microsoft\Windows\CurrentVersion\Run | BobUpdater";
    assert!(found.iter().any(|text| text == bob), "{found:#?}");
    let finding = report
        .findings
        .iter()
        .find(|f| f.value.as_deref() == Some("BobUpdater"))
        .unwrap();
    assert_eq!(finding.account.as_deref(), Some("Bob"));
    assert!(finding.needs_admin);
    assert_eq!(system.hive_releases(), before + 1);
}

#[test]
fn excluded_keys_are_dropped() {
    let (dir, system) = machine();
    let options = Options {
        excludes: vec![
            (
                RegPath::new(
                    Hive::CurrentUser,
                    r"Software\Microsoft\Windows\CurrentVersion\Run",
                ),
                false,
            ),
            (RegPath::new(Hive::CurrentUser, r"Software\Classes"), true),
        ],
        ..defaults()
    };
    let found = shown(&dir, &run(&system, Mode::Safe, false, &options));
    assert!(
        found
            .iter()
            .all(|text| !text.contains("FabrikamUpdater") || text.contains("Uninstall"))
    );
    assert!(
        found
            .iter()
            .all(|text| !text.starts_with(r"HKCU\Software\Classes")),
        "{found:#?}"
    );
    assert_eq!(found.len(), SAFE.len() - 3);
}

#[test]
fn without_administrator_rights_machine_entries_start_unticked() {
    let (_dir, system) = machine();
    system.set_elevated(false);
    let report = run(&system, Mode::Safe, false, &defaults());
    for finding in &report.findings {
        assert_eq!(
            finding.ticked,
            !finding.needs_admin,
            "{}",
            finding.key.display()
        );
    }
}

#[test]
fn the_include_list_offers_a_keys_contents() {
    let (dir, system) = machine();
    let junk = RegPath::new(Hive::CurrentUser, r"Software\Junk");
    system.put(&junk, vec![crate::RawValue::string("a", "1")]);
    system.put(&junk.child("Sub"), vec![]);
    let options = Options {
        includes: vec![(junk, true)],
        ..defaults()
    };
    let request = Request {
        mode: Mode::Custom,
        categories: vec![Category::IncludeList],
        all_accounts: false,
    };
    let report = scan(&system, &request, &options, &Progress::default());
    assert_eq!(
        shown(&dir, &report),
        vec![
            r"HKCU\Software\Junk | a".to_owned(),
            r"HKCU\Software\Junk\Sub".to_owned()
        ]
    );
}

#[test]
fn a_cancelled_scan_says_so() {
    let (_dir, system) = machine();
    let progress = Progress::default();
    progress.cancel();
    let request = Request {
        mode: Mode::Advanced,
        categories: Vec::new(),
        all_accounts: false,
    };
    let report = scan(&system, &request, &defaults(), &progress);
    assert!(report.cancelled);
    assert!(report.findings.is_empty());
    assert!(!paths::NOTES_EXE.is_empty());
}
