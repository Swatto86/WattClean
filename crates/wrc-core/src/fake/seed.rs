//! The test machine: Contoso Notes is installed and healthy; Fabrikam's
//! programs were removed badly and left one broken entry in each category;
//! Northwind left an empty key; a second account, Bob, has a broken
//! startup entry of his own.

use std::fs;
use std::io;

use super::FakeSystem;
use crate::model::{Hive, REG_BINARY, RawValue, RegPath};

/// Text only a build with the fake machine contains; the WebDriver suite
/// refuses to run a binary without it.
pub const FAKE_MARKER: &str = "Fabrikam Updater (test machine)";

/// Where the seeded files are, relative to the sandbox.
pub mod paths {
    pub const NOTES_EXE: &str = r"Program Files\Contoso\Notes\notes.exe";
    pub const NOTES_DLL: &str = r"Program Files\Contoso\Notes\notes.dll";
    pub const SPELL_DLL: &str = r"Program Files\Contoso\Notes\spell.dll";
    pub const NOTES_UNINSTALL: &str = r"Program Files\Contoso\Notes\uninstall.exe";
    pub const UPDATER_EXE: &str = r"Program Files\Fabrikam\Updater\update.exe";
    pub const SHARED_DLL: &str = r"Program Files\Fabrikam\Shared\fabcore.dll";
}

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn hkcu(path: &str) -> RegPath {
    RegPath::new(Hive::CurrentUser, path)
}

fn hklm(path: &str) -> RegPath {
    RegPath::new(Hive::LocalMachine, path)
}

fn s(name: &str, text: &str) -> RawValue {
    RawValue::string(name, text)
}

pub(super) fn seed(system: &FakeSystem) -> io::Result<()> {
    sandbox(system)?;
    let root = system.root().to_path_buf();
    let at = |relative: &str| root.join(relative).to_string_lossy().into_owned();
    let notes = at(paths::NOTES_EXE);
    let updater = at(paths::UPDATER_EXE);
    let fabrikam = at(r"Program Files\Fabrikam");
    seed_startup(system, &notes, &updater, &fabrikam);
    seed_programs(system, &at, &notes, &updater, &fabrikam);
    seed_moderate(system, &at, &fabrikam);
    seed_advanced(system, &fabrikam);
    Ok(())
}

/// The sandbox's folders, Contoso's files and the environment pointing there.
fn sandbox(system: &FakeSystem) -> io::Result<()> {
    let root = system.root().to_path_buf();
    let at = |relative: &str| root.join(relative).to_string_lossy().into_owned();
    for folder in [
        r"Windows\System32",
        r"Windows\SysWOW64",
        r"Windows\Fonts",
        r"ProgramData\Microsoft\Windows\Start Menu\Programs\Startup",
        r"Users\Tester\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup",
        r"Users\Tester\AppData\Local",
        r"Users\Bob\AppData\Roaming",
    ] {
        fs::create_dir_all(root.join(folder))?;
    }
    for file in [
        paths::NOTES_EXE,
        paths::NOTES_DLL,
        paths::SPELL_DLL,
        paths::NOTES_UNINSTALL,
    ] {
        let path = root.join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, b"contoso")?;
    }
    for (name, relative) in [
        ("SystemRoot", "Windows"),
        ("windir", "Windows"),
        ("ProgramFiles", "Program Files"),
        ("ProgramData", "ProgramData"),
        ("USERPROFILE", r"Users\Tester"),
        ("APPDATA", r"Users\Tester\AppData\Roaming"),
        ("LOCALAPPDATA", r"Users\Tester\AppData\Local"),
    ] {
        system.set_env(name, &at(relative));
    }
    Ok(())
}

fn seed_startup(system: &FakeSystem, notes: &str, updater: &str, fabrikam: &str) {
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
        vec![
            s("CurrentBuildNumber", "26100"),
            s("ProductName", "Windows 11 Pro"),
        ],
    );
    system.put(
        &hkcu(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"),
        vec![RawValue::dword("HideFileExt", 1)],
    );

    // Startup items: one healthy, one missing, one on a drive that is not
    // attached, one inside Windows, and a switch for a removed item.
    system.put(
        &hkcu(RUN),
        vec![
            s("ContosoNotes", &format!("\"{notes}\" /tray")),
            s("FabrikamUpdater", &format!("\"{updater}\" /background")),
            s("PortableTool", r"Q:\Portable\tool.exe"),
            s("OldWindowsHelper", r"%SystemRoot%\oldhelper.exe"),
        ],
    );
    system.put(
        &hklm(r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run"),
        vec![s(
            "FabrikamAgent",
            &format!(r"{fabrikam}\Agent\agent.exe -silent"),
        )],
    );
    let switch = RawValue {
        name: "GhostApp".into(),
        kind: REG_BINARY,
        bytes: vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    };
    let mut kept = switch.clone();
    kept.name = "ContosoNotes".into();
    system.put(
        &hkcu(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"),
        vec![switch, kept],
    );
}

fn seed_programs(
    system: &FakeSystem,
    at: &dyn Fn(&str) -> String,
    notes: &str,
    updater: &str,
    fabrikam: &str,
) {
    let app_paths = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";
    system.put(
        &hklm(&format!(r"{app_paths}\fabupdate.exe")),
        vec![s("", updater)],
    );
    system.put(
        &hklm(&format!(r"{app_paths}\notes.exe")),
        vec![s("", notes)],
    );
    system.put(
        &hkcu(r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"),
        vec![
            s(&format!("{updater}.FriendlyAppName"), FAKE_MARKER),
            s(&format!("{notes}.FriendlyAppName"), "Contoso Notes"),
        ],
    );
    let shared = r"SOFTWARE\Microsoft\Windows\CurrentVersion\SharedDLLs";
    system.put(
        &hklm(shared),
        vec![
            RawValue::dword(&at(paths::SHARED_DLL), 1),
            RawValue::dword(&at(paths::NOTES_DLL), 1),
            RawValue::dword(&at(paths::SPELL_DLL), 0),
        ],
    );
    system.put(
        &hkcu(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\FabrikamUpdater"),
        vec![
            s("DisplayName", "Fabrikam Updater"),
            s(
                "UninstallString",
                &format!(r#""{fabrikam}\Updater\uninstall.exe""#),
            ),
            s("InstallLocation", &format!(r"{fabrikam}\Updater")),
        ],
    );
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ContosoNotes"),
        vec![
            s("DisplayName", "Contoso Notes"),
            s(
                "UninstallString",
                &format!("\"{}\"", at(paths::NOTES_UNINSTALL)),
            ),
        ],
    );
    system.put(
        &hkcu(r"Software\Classes\Directory\shell\FabrikamScan"),
        vec![s("", "Scan with Fabrikam")],
    );
    system.put(
        &hkcu(r"Software\Classes\Directory\shell\FabrikamScan\command"),
        vec![s("", &format!(r#""{fabrikam}\Scan\scan.exe" "%1""#))],
    );
}

fn seed_moderate(system: &FakeSystem, at: &dyn Fn(&str) -> String, fabrikam: &str) {
    let clsid = r"SOFTWARE\Classes\CLSID";
    system.put(
        &hklm(&format!(
            r"{clsid}\{{F00DFACE-0000-4000-8000-000000000001}}"
        )),
        vec![s("", "Fabrikam Shell Extension")],
    );
    system.put(
        &hklm(&format!(
            r"{clsid}\{{F00DFACE-0000-4000-8000-000000000001}}\InprocServer32"
        )),
        vec![s("", &at(paths::SHARED_DLL))],
    );
    // Installed by TrustedInstaller, so Windows refuses to remove it
    // unless the app takes ownership first.
    let protected = hklm(&format!(
        r"{clsid}\{{F00DFACE-0000-4000-8000-000000000004}}"
    ));
    system.put(&protected, vec![s("", "Fabrikam Protected Handler")]);
    system.put(
        &protected.child("InprocServer32"),
        vec![s("", &at(r"Program Files\Fabrikam\Shared\fabshell.dll"))],
    );
    system.protect(&protected);
    system.put(
        &hklm(&format!(
            r"{clsid}\{{C0FFEE00-0000-4000-8000-000000000002}}\InprocServer32"
        )),
        vec![s("", &at(paths::NOTES_DLL))],
    );
    system.put(
        &hklm(&format!(
            r"{clsid}\{{C0FFEE00-0000-4000-8000-000000000003}}\InprocServer32"
        )),
        vec![s("", "mscoree.dll")],
    );
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\Folders"),
        vec![
            s(&format!("{fabrikam}\\"), ""),
            s(&at(r"Program Files\Contoso\"), ""),
        ],
    );
    system.put(
        &hklm(r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules"),
        vec![s(
            "{F00DFACE-0000-4000-8000-0000000000FF}",
            &format!(
                "v2.30|Action=Allow|Active=TRUE|Dir=In|App={}|Name=Fabrikam Updater|",
                at(paths::UPDATER_EXE)
            ),
        )],
    );
    system.put(
        &hkcu(r"AppEvents\Schemes\Apps\Fabrikam\Alert\.Current"),
        vec![s("", &format!(r"{fabrikam}\Sounds\alert.wav"))],
    );
    system.put(
        &hkcu(r"Software\Classes\.fabdoc"),
        vec![s("", "Fabrikam.Document")],
    );
    system.put(&hkcu(r"Software\Classes\.txt"), vec![s("", "txtfile")]);
    system.put(
        &hklm(r"SOFTWARE\Classes\txtfile\shell\open\command"),
        vec![s("", "notepad.exe %1")],
    );
}

fn seed_advanced(system: &FakeSystem, fabrikam: &str) {
    system.put(
        &hklm(r"SYSTEM\CurrentControlSet\Services\FabrikamSvc"),
        vec![
            RawValue::dword("Type", 0x10),
            s("DisplayName", "Fabrikam Update Service"),
            s("ImagePath", &format!(r#""{fabrikam}\Service\fabsvc.exe""#)),
        ],
    );
    system.put(&hkcu(r"Software\Northwind"), vec![]);
    system.put(
        &hkcu(r"Software\Classes\Fabrikam.Report\shell\open\command"),
        vec![s("", &format!(r#""{fabrikam}\Report\report.exe" "%1""#))],
    );
    system.put(
        &hkcu(r"Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU"),
        vec![s("a", r"regedit\1"), s("MRUList", "a")],
    );
    // Bob's own registry, reached as another account's hive.
    system.put(
        &RegPath::account_root("Bob").child(RUN),
        vec![s(
            "BobUpdater",
            &format!(r#""{fabrikam}\Updater\update.exe""#),
        )],
    );
}
