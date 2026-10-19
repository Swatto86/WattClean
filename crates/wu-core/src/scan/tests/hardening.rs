//! Things a scan must leave alone because they are shared, newer or
//! another program's.

use super::{Machine, has, machine, shown, uninstall_and_scan};
use crate::model::{Hive, RawValue, RegPath};
use crate::programs::UNINSTALL;
use crate::scan::{Mode, ScanReport};
use crate::system::System;

fn hklm(path: &str) -> RegPath {
    RegPath::new(Hive::LocalMachine, path)
}

fn text(name: &str, value: &str) -> RawValue {
    RawValue::string(name, value)
}

fn offered(report: &ScanReport, fragment: &str) -> bool {
    shown(report).iter().any(|item| item.contains(fragment))
}

/// Acme Widget with its website set to `site`, and `.url` files on the Desktop.
fn scan_with_site(site: &str, shortcuts: &[(&str, &str)]) -> (Machine, ScanReport) {
    let machine = machine();
    let key = hklm(&format!(r"{UNINSTALL}\AcmeWidget"));
    machine
        .system
        .write_value(&key, &text("URLInfoAbout", site))
        .unwrap();
    for (name, url) in shortcuts {
        let file = machine.folders.desktop.join(format!("{name}.url"));
        std::fs::write(file, format!("[InternetShortcut]\r\nURL={url}\r\n")).unwrap();
    }
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Moderate);
    (machine, report)
}

#[test]
fn a_web_shortcut_on_a_shared_host_is_not_offered_by_host_alone() {
    let (_machine, report) = scan_with_site(
        "https://github.com/acme/widget",
        &[
            ("My GitHub", "https://github.com/someone/else"),
            ("Releases", "https://github.com/Acme/Widget/releases"),
        ],
    );
    assert!(!has(&report, "My GitHub.url"), "{:#?}", shown(&report));
    assert!(has(&report, "Releases.url"), "{:#?}", shown(&report));

    let (_machine, report) = scan_with_site(
        "https://github.com",
        &[("My GitHub", "https://github.com/someone/else")],
    );
    assert!(!has(&report, "My GitHub.url"), "{:#?}", shown(&report));
}

#[test]
fn a_web_shortcut_must_lie_on_the_programs_own_path() {
    let (_machine, report) = scan_with_site(
        "https://acme.example/widget",
        &[
            ("Store", "https://acme.example/store"),
            ("Widgets", "https://acme.example/widgets"),
            ("Support", "https://www.acme.example/widget/help"),
        ],
    );
    assert!(!has(&report, "Store.url"));
    assert!(!has(&report, "Widgets.url"), "a path is matched whole");
    assert!(has(&report, "Support.url"), "{:#?}", shown(&report));
}

fn widget_exe(machine: &Machine) -> String {
    machine
        .folders
        .program_files
        .join(r"Acme\Widget\widget.exe")
        .display()
        .to_string()
}

#[test]
fn windows_own_file_types_are_never_offered_whole() {
    let machine = machine();
    let exe = widget_exe(&machine);
    for class in ["txtfile", "htmlfile", "https", "mailto", "batfile"] {
        machine.system.put(
            &hklm(&format!(r"SOFTWARE\Classes\{class}\shell\open\command")),
            vec![text("", &format!("\"{exe}\" \"%1\""))],
        );
    }
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Advanced);
    for class in ["txtfile", "htmlfile", "https", "mailto", "batfile"] {
        assert!(
            !offered(&report, &format!(r"Classes\{class}")),
            "{class} offered: {:#?}",
            shown(&report)
        );
    }
    assert!(
        has(&report, r"Classes\AcmeWidget.Document"),
        "its own stays"
    );
}

#[test]
fn a_deep_search_leaves_windows_settings_and_other_programs_keys() {
    let machine = machine();
    let exe = widget_exe(&machine);
    let winlogon = hklm(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon");
    machine
        .system
        .put(&winlogon, vec![text("Userinit", &format!("{exe},"))]);
    let theirs = hklm(r"SOFTWARE\AcmeWidgetHelper\Config");
    machine.system.put(&theirs, vec![text("Plugin", &exe)]);
    let stray = hklm(r"SOFTWARE\Misc\Thing");
    machine.system.put(&stray, vec![text("Exe", &exe)]);
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Advanced);
    assert!(!offered(&report, "Winlogon"), "{:#?}", shown(&report));
    assert!(
        !offered(&report, "AcmeWidgetHelper"),
        "{:#?}",
        shown(&report)
    );
    assert!(has(&report, r"Misc\Thing → Exe"), "{:#?}", shown(&report));
}

#[test]
fn registrations_of_a_program_installed_inside_it_are_left_alone() {
    let machine = machine();
    let addons = machine.folders.program_files.join(r"Acme\Widget\addons");
    let plug = addons.join("plug.exe");
    std::fs::create_dir_all(&addons).unwrap();
    std::fs::write(&plug, b"x").unwrap();
    let plug = plug.display().to_string();
    machine.system.put(
        &hklm(&format!(r"{UNINSTALL}\AcmePluginPack")),
        vec![
            text("DisplayName", "Acme Plugin Pack"),
            text("InstallLocation", &addons.display().to_string()),
            text("UninstallString", &plug),
        ],
    );
    machine.system.put(
        &hklm(r"SYSTEM\CurrentControlSet\Services\AcmePluginSvc"),
        vec![RawValue::expand_string("ImagePath", &plug)],
    );
    let run = hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run");
    machine
        .system
        .write_value(&run, &text("PluginPack", &plug))
        .unwrap();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Moderate);
    assert!(!offered(&report, "AcmePluginSvc"), "{:#?}", shown(&report));
    assert!(!offered(&report, "PluginPack"), "{:#?}", shown(&report));
    assert!(has(&report, r"Services\AcmeWidgetSvc"), "its own stays");
}

#[test]
fn a_type_library_shared_with_another_program_loses_only_its_versions() {
    let machine = machine();
    let exe = widget_exe(&machine);
    let other = machine.folders.program_files.join(r"Contoso\contoso.dll");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::write(&other, b"x").unwrap();
    let shared = "{AAAA0001-0000-0000-0000-000000000001}";
    let own = "{AAAA0002-0000-0000-0000-000000000002}";
    let lib = |guid: &str, version: &str| {
        hklm(&format!(
            r"SOFTWARE\Classes\TypeLib\{guid}\{version}\0\win32"
        ))
    };
    machine
        .system
        .put(&lib(shared, "1.0"), vec![text("", &exe)]);
    machine.system.put(
        &lib(shared, "2.0"),
        vec![text("", &other.display().to_string())],
    );
    machine.system.put(&lib(own, "1.0"), vec![text("", &exe)]);
    machine
        .system
        .put(&lib(own, "1.1"), vec![text("", r"C:\Gone\old.dll")]);
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Moderate);
    assert!(
        has(&report, &format!(r"TypeLib\{shared}\1.0")),
        "{:#?}",
        shown(&report)
    );
    assert!(!has(&report, &format!(r"TypeLib\{shared}")));
    assert!(!offered(&report, &format!(r"{shared}\2.0")));
    assert!(
        has(&report, &format!(r"TypeLib\{own}")),
        "a library wholly its own goes whole: {:#?}",
        shown(&report)
    );
}

#[test]
fn another_accounts_live_copy_of_the_program_is_not_offered() {
    let machine = machine();
    let local = machine
        .folders
        .users
        .join(r"other\AppData\Local\Acme Widget");
    std::fs::create_dir_all(&local).unwrap();
    let updater = local.join("Update.exe");
    std::fs::write(&updater, b"x").unwrap();
    let entry = RegPath::account_root("other").child(&format!(r"{UNINSTALL}\AcmeWidgetUser"));
    machine
        .system
        .write_value(
            &entry,
            &text("UninstallString", &updater.display().to_string()),
        )
        .unwrap();
    let report = uninstall_and_scan(&machine, "Acme Widget 2.1 (x64)", Mode::Advanced);
    assert!(!offered(&report, "AcmeWidgetUser"), "{:#?}", shown(&report));
    assert!(
        !offered(&report, r"other\AppData\Local\Acme Widget"),
        "{:#?}",
        shown(&report)
    );
    assert!(
        has(&report, r"other\AppData\Roaming\Acme Widget"),
        "the account's data folder is still a leftover"
    );
}
