//! The guard: what may never be offered, whatever matched.

use std::path::Path;

use super::super::guard::{key_allowed, path_allowed, value_allowed};
use super::{Quiet, machine};
use crate::System;
use crate::identity::Identity;
use crate::model::{Hive, RegPath};
use crate::programs::list_programs;
use crate::scan::{Context, OtherProgram};

#[test]
fn the_guard_keeps_windows_and_other_programs_safe() {
    let machine = machine();
    let system: &dyn System = &machine.system;
    let programs = list_programs(system, true);
    let widget = programs
        .iter()
        .find(|p| p.name.starts_with("Acme Widget 2"))
        .unwrap();
    let others = OtherProgram::all(system, &machine.folders, &programs, Some(widget));
    let context = Context {
        system,
        folders: &machine.folders,
        others: &others,
        progress: &Quiet,
        log: None,
        protected: &[],
    };
    let hklm = |path: &str| RegPath::new(Hive::LocalMachine, path);
    for blocked in [
        "SOFTWARE",
        r"SOFTWARE\Microsoft",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion",
        r"SOFTWARE\Microsoft\Office",
        r"SOFTWARE\Classes\.wdg",
        r"SOFTWARE\Classes\CLSID",
        r"SOFTWARE\Classes\Directory\shell\acme",
        r"SOFTWARE\Classes\*\shellex\ContextMenuHandlers",
        r"SOFTWARE\Classes\*",
        r"SYSTEM\CurrentControlSet\Services",
        r"SYSTEM\CurrentControlSet\Control\Acme",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\AcmeHelper",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\Clients\Media",
        r"SOFTWARE\WOW6432Node\Clients",
        r"SOFTWARE\WOW6432Node\Clients\Mail",
        r"SYSTEM\CurrentControlSet\Services\EventLog\Application",
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tasks",
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree\Microsoft\Windows\Maintenance\WinSAT",
    ] {
        assert!(!key_allowed(&context, &hklm(blocked)), "{blocked} allowed");
    }
    for allowed in [
        r"SOFTWARE\Acme\Widget",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\widget.exe",
        r"SOFTWARE\Classes\CLSID\{11111111-2222-3333-4444-555555555555}",
        r"SYSTEM\CurrentControlSet\Services\AcmeWidgetSvc",
        r"SOFTWARE\Classes\*\shellex\ContextMenuHandlers\AcmeWidget",
        r"SOFTWARE\Clients\Media\AcmeWidget",
        r"SOFTWARE\WOW6432Node\Clients\Mail\AcmeWidget",
        r"SYSTEM\CurrentControlSet\Services\EventLog\Application\AcmeWidget",
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tasks\{AAAAAAAA-1111-2222-3333-000000000001}",
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree\Acme\Widget Update",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Management\ARPCache\AcmeWidget",
        r"SOFTWARE\Classes\Installer\Dependencies\WoodgroveRuntime",
    ] {
        assert!(key_allowed(&context, &hklm(allowed)), "{allowed} refused");
    }
}

#[test]
fn the_apps_own_folders_and_package_caches_stay_out_of_reach() {
    let machine = machine();
    let system: &dyn System = &machine.system;
    let programs = list_programs(system, true);
    let widget = programs
        .iter()
        .find(|p| p.name.starts_with("Acme Widget 2"))
        .unwrap();
    let others = OtherProgram::all(system, &machine.folders, &programs, Some(widget));
    let own = vec![machine.dir.path().join("data")];
    let context = Context {
        system,
        folders: &machine.folders,
        others: &others,
        progress: &Quiet,
        log: None,
        protected: &own,
    };
    let folders = &machine.folders;
    let identity = Identity::of_program(system, folders, widget);
    for blocked in [
        machine.dir.path().join("data"),
        machine.dir.path().join(r"data\backups\x"),
        machine.dir.path().to_path_buf(),
        folders.program_data.join("Package Cache"),
        folders.program_data.join(r"Package Cache\{X}"),
    ] {
        assert!(
            !path_allowed(&context, &identity, &blocked),
            "{} allowed",
            blocked.display()
        );
    }
    let bundle = programs
        .iter()
        .find(|p| p.name == "Woodgrove Runtime")
        .unwrap();
    let bundle_identity = Identity::of_program(system, folders, bundle);
    assert!(bundle_identity.bundle.is_some());
    assert!(path_allowed(
        &context,
        &bundle_identity,
        &folders.program_data.join(r"Package Cache\{X}")
    ));
    assert!(!path_allowed(
        &context,
        &bundle_identity,
        &folders.program_data.join(r"Package Cache\{X}\sub")
    ));
    assert!(!path_allowed(
        &context,
        &bundle_identity,
        &folders.program_data.join("Package Cache")
    ));
}

#[test]
fn another_accounts_hive_is_judged_like_the_current_users() {
    let machine = machine();
    let system: &dyn System = &machine.system;
    let programs = list_programs(system, true);
    let widget = programs
        .iter()
        .find(|p| p.name.starts_with("Acme Widget 2"))
        .unwrap();
    let others = OtherProgram::all(system, &machine.folders, &programs, Some(widget));
    let context = Context {
        system,
        folders: &machine.folders,
        others: &others,
        progress: &Quiet,
        log: None,
        protected: &[],
    }; // Another account's hive is judged like HKCU, and only through its
    // `Account-<name>` address; a raw SID or the mount itself is never touched.
    let hku = |path: &str| RegPath::new(Hive::Users, path);
    for allowed in [
        r"Account-other\Software\Acme\Widget",
        r"Account-other_Classes\CLSID\{11111111-2222-3333-4444-555555555555}",
        r"Account-other\Software\Microsoft\Windows\CurrentVersion\Uninstall\AcmeWidgetUser",
    ] {
        assert!(key_allowed(&context, &hku(allowed)), "{allowed} refused");
    }
    for blocked in [
        "Account-other",
        r"Account-other\Software",
        r"Account-other\Software\Microsoft",
        r"Account-other\Software\Microsoft\Windows\CurrentVersion\Run",
        r"Account-other_Classes\CLSID",
        r"Account-other_Classes\.wdg",
        r"S-1-5-21-1\Software\Acme\Widget",
    ] {
        assert!(!key_allowed(&context, &hku(blocked)), "{blocked} allowed");
    }
    assert!(value_allowed(
        &context,
        &hku(r"Account-other\Software\Microsoft\Windows\CurrentVersion\Run")
    ));
    assert!(!value_allowed(&context, &hku(r"Account-other\Environment")));
    assert!(!value_allowed(&context, &hku(r"S-1-5-21-1\Software\Acme")));

    let identity = Identity::named("Acme Widget", None, Vec::new());
    let folders = &machine.folders;
    for blocked in [
        folders.program_files.clone(),
        folders.program_files.join("Acme").join(".."),
        folders.windows.join("Acme"),
        folders.documents.join("Acme Widget"),
        folders.program_data.join(r"Microsoft\Acme"),
        folders.program_files.join(r"Acme\Helper\data"),
        folders.program_files.join("Acme"),
        folders.program_files.join("Windows Mail"),
        folders.start_menu_programs.join("Startup"),
        Path::new(r"relative\Acme").to_path_buf(),
        folders
            .windows
            .join(r"System32\Tasks\Microsoft\Windows\Maintenance\WinSAT"),
        folders.windows.join(r"System32\Tasks\Acme"),
        folders.users.join(r"other\AppData\Roaming"),
        folders.users.join(r"other\Documents\acme widget notes.txt"),
        folders.users.join("other"),
        folders.users.join(r"other\AppData\Local\Programs"),
        folders.users.join(r"other\AppData\Local\Packages"),
        folders
            .users
            .join(r"other\AppData\Local\Packages\Acme.Widget_1a2b"),
        folders.users.join(r"other\AppData\Local\Temp"),
        folders.users.join(r"other\AppData\Local\Package Cache"),
    ] {
        assert!(
            !path_allowed(&context, &identity, &blocked),
            "{} allowed",
            blocked.display()
        );
    }
    for allowed in [
        folders.program_files.join(r"Acme\Widget"),
        folders.app_data.join("Acme Widget"),
        folders.common_start_menu_programs.join("Acme Widget"),
        folders.desktop.join("Acme Widget.lnk"),
        folders.windows.join(r"System32\Tasks\Acme\Widget Update"),
        folders.users.join(r"other\AppData\Roaming\Acme Widget"),
        folders
            .users
            .join(r"other\AppData\Local\Programs\Acme Widget"),
        folders.users.join(r"other\AppData\Local\Temp\Acme Widget"),
        folders.public_desktop.join("Acme Widget.lnk"),
    ] {
        assert!(
            path_allowed(&context, &identity, &allowed),
            "{} refused",
            allowed.display()
        );
    }
}
