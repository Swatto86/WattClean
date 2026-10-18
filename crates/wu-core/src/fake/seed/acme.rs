//! The Acme programs: a widget whose uninstaller leaves plenty behind, and
//! a helper from the same publisher that must survive the widget's removal.

use std::io;
use std::path::Path;

use super::super::{Effect, FakeSystem};
use super::{UNINSTALL, file, hkcu, hklm, s};
use crate::model::RawValue;

pub(super) fn seed_widget(system: &FakeSystem, root: &Path) -> io::Result<()> {
    let dir = root.join(r"Program Files\Acme\Widget");
    let exe = dir.join("widget.exe").display().to_string();
    for (name, size) in [
        ("widget.exe", 4096),
        ("uninstall.exe", 1024),
        ("widget-svc.exe", 2048),
        (r"plugins\core.dll", 2048),
        ("settings.ini", 100),
    ] {
        file(&dir.join(name), size)?;
    }
    file(
        &root.join(r"Users\tester\AppData\Roaming\Acme\Widget\config.json"),
        300,
    )?;
    file(
        &root.join(r"Users\tester\AppData\Local\Acme Widget\cache.bin"),
        5000,
    )?;
    file(&root.join(r"ProgramData\Acme\Widget\license.dat"), 64)?;
    file(
        &root.join(r"Users\tester\AppData\Local\widget-svc\state.db"),
        32,
    )?;
    file(
        &root
            .join(r"ProgramData\Microsoft\Windows\Start Menu\Programs\Acme Widget\Acme Widget.lnk"),
        64,
    )?;
    file(&root.join(r"Users\Public\Desktop\Acme Widget.lnk"), 64)?;
    file(
        &root.join(r"Users\tester\AppData\Local\Temp\AcmeWidget_install.log"),
        64,
    )?;
    seed_widget_keys(system, &dir, &exe);
    super::windows::seed_widget_windows(system, root, &dir, &exe)?;
    super::accounts::seed_other_account(system, root, &exe)?;
    Ok(())
}

/// The widget's registrations: its own keys and Windows' records of its files.
fn seed_widget_keys(system: &FakeSystem, dir: &Path, exe: &str) {
    seed_applications(system, exe);
    let key = hklm(&format!(r"{UNINSTALL}\AcmeWidget"));
    system.put(
        &key,
        vec![
            s("DisplayName", "Acme Widget 2.1 (x64)"),
            s("DisplayVersion", "2.1.0"),
            s("Publisher", "Acme Software Ltd."),
            s("InstallDate", "20260304"),
            RawValue::dword("EstimatedSize", 20_480),
            s("InstallLocation", &format!("{}\\", dir.display())),
            RawValue::expand_string(
                "UninstallString",
                r#""%ProgramFiles%\Acme\Widget\uninstall.exe" /U"#,
            ),
            s("DisplayIcon", &format!("{exe},0")),
            s("URLInfoAbout", "https://acme.example/widget"),
        ],
    );
    system.put(
        &hklm(r"SOFTWARE\Acme\Widget"),
        vec![s("InstallDir", &dir.display().to_string())],
    );
    system.put(
        &hkcu(r"Software\Acme\Widget\Recent"),
        vec![s("File1", r"C:\notes.wdg")],
    );
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\widget.exe"),
        vec![s("", exe)],
    );
    system.put(
        &hkcu(r"Software\Microsoft\Windows\CurrentVersion\Run"),
        vec![
            s("AcmeWidget", &format!("\"{exe}\" --tray")),
            s("OtherTool", r"C:\Tools\other.exe"),
        ],
    );
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\SharedDLLs"),
        vec![RawValue::dword(
            &dir.join(r"plugins\core.dll").display().to_string(),
            1,
        )],
    );
    let clsid = r"SOFTWARE\Classes\CLSID\{11111111-2222-3333-4444-555555555555}";
    system.put(&hklm(clsid), vec![s("", "Acme Widget Shell Extension")]);
    system.put(
        &hklm(&format!(r"{clsid}\InprocServer32")),
        vec![s("", &dir.join(r"plugins\core.dll").display().to_string())],
    );
    system.put(
        &hklm(r"SOFTWARE\Classes\CLSID\{99999999-0000-0000-0000-000000000000}\InprocServer32"),
        vec![s("", r"C:\Windows\System32\shell32.dll")],
    );
    system.put(
        &hklm(r"SOFTWARE\Classes\AcmeWidget.Document\shell\open\command"),
        vec![s("", &format!("\"{exe}\" \"%1\""))],
    );
    system.put(
        &hklm(r"SOFTWARE\Classes\*\shellex\ContextMenuHandlers\AcmeWidget"),
        vec![s("", "{11111111-2222-3333-4444-555555555555}")],
    );
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved"),
        vec![
            s(
                "{11111111-2222-3333-4444-555555555555}",
                "Acme Widget Shell Extension",
            ),
            s("{99999999-0000-0000-0000-000000000000}", "Windows"),
        ],
    );
    system.put(
        &hklm(r"SOFTWARE\Classes\.wdg"),
        vec![s("", "AcmeWidget.Document")],
    );
    system.put(
        &hkcu(r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"),
        vec![s(&format!("{exe}.FriendlyAppName"), "Acme Widget")],
    );
    system.put(
        &hklm(r"SYSTEM\CurrentControlSet\Services\AcmeWidgetSvc"),
        vec![RawValue::expand_string(
            "ImagePath",
            &format!("\"{}\"", dir.join("widget-svc.exe").display()),
        )],
    );
    system.put(
        &hklm(r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules"),
        vec![s(
            "{AAAAAAAA-0000-0000-0000-000000000001}",
            &format!("v2.31|Action=Allow|Active=TRUE|Dir=In|App={exe}|Name=Acme Widget|"),
        )],
    );
    system.add_effect(Effect {
        command_contains: r"acme\widget\uninstall.exe".into(),
        delete_keys: vec![key],
        delete_paths: vec![dir.join("widget.exe"), dir.join("uninstall.exe")],
    });
}

pub(super) fn seed_helper(system: &FakeSystem, root: &Path) -> io::Result<()> {
    let dir = root.join(r"Program Files\Acme\Helper");
    file(&dir.join("helper.exe"), 512)?;
    file(&dir.join("unins000.exe"), 512)?;
    file(
        &root.join(
            r"ProgramData\Microsoft\Windows\Start Menu\Programs\Acme Widget Helper\Helper.lnk",
        ),
        64,
    )?;
    let key = hklm(&format!(r"{UNINSTALL}\AcmeHelper"));
    system.put(
        &key,
        vec![
            s("DisplayName", "Acme Widget Helper"),
            s("Publisher", "Acme Software Ltd."),
            s("InstallLocation", &dir.display().to_string()),
            // Unquoted with a space in the path, as some installers write it.
            s(
                "UninstallString",
                &dir.join("unins000.exe").display().to_string(),
            ),
        ],
    );
    system.put(&hklm(r"SOFTWARE\Acme\Helper"), vec![s("Version", "1.0")]);
    system.add_effect(Effect {
        command_contains: r"acme\helper\unins000.exe".into(),
        delete_keys: vec![key, hklm(r"SOFTWARE\Acme\Helper")],
        delete_paths: vec![dir],
    });
    Ok(())
}

/// `Applications` entries: the widget's own, and a same-named one of another program.
fn seed_applications(system: &FakeSystem, exe: &str) {
    let open = |name: &str, command: String| {
        let key = format!(r"SOFTWARE\Classes\Applications\{name}\shell\open\command");
        system.put(&hklm(&key), vec![s("", &command)]);
    };
    open("widget.exe", format!("\"{exe}\" \"%1\""));
    // Another program's uninstaller with the same file name: never ours.
    open("uninstall.exe", r"C:\Other\uninstall.exe".into());
}
