//! Windows' deeper records of the Acme widget: a scheduled task, an event
//! log source, a font, a startup approval, a default-program registration,
//! open-with lists, COM interfaces, Explorer's uninstall cache, a web
//! shortcut, a nested Start Menu shortcut and another account's data. Each
//! comes with a neighbour that is not the widget's and must stay.

use std::io;
use std::path::Path;

use super::super::FakeSystem;
use super::{file, hkcu, hklm, s};
use crate::model::{REG_BINARY, RawValue};
use crate::scan::shortcut::link_bytes;

pub const TASK_ID: &str = "{AAAAAAAA-1111-2222-3333-000000000001}";
pub const INTERFACE_ID: &str = "{BBBBBBBB-1111-2222-3333-000000000001}";
pub const APP_ID: &str = "{CCCCCCCC-1111-2222-3333-000000000001}";
const CLSID: &str = "{11111111-2222-3333-4444-555555555555}";
const TASK_CACHE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache";
const EXPLORER: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer";

fn binary(name: &str, bytes: Vec<u8>) -> RawValue {
    RawValue {
        name: name.to_owned(),
        kind: REG_BINARY,
        bytes,
    }
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

pub(super) fn seed_widget_windows(
    system: &FakeSystem,
    root: &Path,
    dir: &Path,
    exe: &str,
) -> io::Result<()> {
    scheduled_tasks(system, root, exe)?;
    system.put(
        &hklm(r"SYSTEM\CurrentControlSet\Services\EventLog\Application\AcmeWidget"),
        vec![RawValue::expand_string("EventMessageFile", exe)],
    );
    system.put(
        &hklm(r"SYSTEM\CurrentControlSet\Services\EventLog\Application\Windows Error Reporting"),
        vec![RawValue::expand_string(
            "EventMessageFile",
            r"%SystemRoot%\System32\wersvc.dll",
        )],
    );
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"),
        vec![
            s(
                "Acme Sans (TrueType)",
                &dir.join(r"fonts\acme.ttf").display().to_string(),
            ),
            s("Arial (TrueType)", "arial.ttf"),
        ],
    );
    system.put(
        &hkcu(&format!(r"{EXPLORER}\StartupApproved\Run")),
        vec![
            binary("AcmeWidget", vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            binary("OtherTool", vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        ],
    );
    default_programs(system, exe);
    open_with(system);
    system.put(
        &hklm(&format!(r"SOFTWARE\Classes\Interface\{INTERFACE_ID}")),
        vec![s("", "IWidget")],
    );
    system.put(
        &hklm(&format!(
            r"SOFTWARE\Classes\Interface\{INTERFACE_ID}\ProxyStubClsid32"
        )),
        vec![s("", CLSID)],
    );
    system.put(
        &hklm(
            r"SOFTWARE\Classes\Interface\{00020400-0000-0000-C000-000000000046}\ProxyStubClsid32",
        ),
        vec![s("", "{00020420-0000-0000-C000-000000000046}")],
    );
    system.put(
        &hklm(&format!(r"SOFTWARE\Classes\CLSID\{CLSID}")),
        vec![s("AppID", APP_ID)],
    );
    system.put(
        &hklm(&format!(r"SOFTWARE\Classes\AppID\{APP_ID}")),
        vec![s("", "Acme Widget")],
    );
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Management\ARPCache\AcmeWidget"),
        vec![RawValue::dword("SlowInfoCache", 1)],
    );
    let desktop = root.join(r"Users\tester\OneDrive\Desktop");
    std::fs::create_dir_all(&desktop)?;
    std::fs::write(
        desktop.join("Help and support.url"),
        "[InternetShortcut]\r\nURL=https://www.acme.example/widget/help\r\n",
    )?;
    std::fs::write(
        desktop.join("Bank.url"),
        "[InternetShortcut]\r\nURL=https://bank.example/\r\n",
    )?;
    let menu = root.join(r"ProgramData\Microsoft\Windows\Start Menu\Programs\Acme\Tools");
    std::fs::create_dir_all(&menu)?;
    std::fs::write(menu.join("Widget Tools.lnk"), link_bytes(exe))?;
    file(
        &root.join(r"Users\other\AppData\Roaming\Acme Widget\prefs.json"),
        40,
    )?;
    file(
        &root.join(r"Users\other\Documents\acme widget notes.txt"),
        40,
    )?;
    Ok(())
}

/// The widget's update task, and one of Windows' own beside it.
fn scheduled_tasks(system: &FakeSystem, root: &Path, exe: &str) -> io::Result<()> {
    let tasks = hklm(&format!(r"{TASK_CACHE}\Tasks"));
    let mut actions = vec![3, 0, 0, 0];
    actions.extend(utf16(exe));
    actions.extend([0, 0]);
    actions.extend(utf16("--update"));
    system.put(
        &tasks.child(TASK_ID),
        vec![
            s("Path", r"\Acme\Widget Update"),
            binary("Actions", actions),
        ],
    );
    system.put(
        &hklm(&format!(r"{TASK_CACHE}\Tree\Acme\Widget Update")),
        vec![s("Id", TASK_ID)],
    );
    system.put(&hklm(&format!(r"{TASK_CACHE}\Logon\{TASK_ID}")), Vec::new());
    let windows_task = "{DDDDDDDD-1111-2222-3333-000000000001}";
    let mut windows_actions = vec![3, 0, 0, 0];
    windows_actions.extend(utf16(r"%SystemRoot%\System32\sc.exe"));
    system.put(
        &tasks.child(windows_task),
        vec![
            s("Path", r"\Microsoft\Windows\Maintenance\WinSAT"),
            binary("Actions", windows_actions),
        ],
    );
    system.put(
        &hklm(&format!(
            r"{TASK_CACHE}\Tree\Microsoft\Windows\Maintenance\WinSAT"
        )),
        vec![s("Id", windows_task)],
    );
    let task_dir = root.join(r"Windows\System32\Tasks\Acme");
    std::fs::create_dir_all(&task_dir)?;
    let mut definition = vec![0xFF, 0xFE];
    definition.extend(utf16(&format!(
        "<?xml version=\"1.0\"?><Task><Actions><Exec><Command>{exe}</Command></Exec></Actions></Task>"
    )));
    std::fs::write(task_dir.join("Widget Update"), definition)?;
    let windows_dir = root.join(r"Windows\System32\Tasks\Microsoft\Windows\Maintenance");
    std::fs::create_dir_all(&windows_dir)?;
    std::fs::write(windows_dir.join("WinSAT"), b"<Task/>")?;
    Ok(())
}

fn default_programs(system: &FakeSystem, exe: &str) {
    system.put(
        &hklm(r"SOFTWARE\Clients\Media\AcmeWidget\shell\open\command"),
        vec![s("", &format!("\"{exe}\""))],
    );
    system.put(
        &hklm(r"SOFTWARE\Clients\Media\AcmeWidget\Capabilities"),
        Vec::new(),
    );
    system.put(
        &hklm(r"SOFTWARE\Clients\Media\WMP\shell\open\command"),
        vec![s("", r"C:\Program Files\Windows Media Player\wmplayer.exe")],
    );
    system.put(
        &hklm(r"SOFTWARE\RegisteredApplications"),
        vec![
            s(
                "Acme Widget",
                r"Software\Clients\Media\AcmeWidget\Capabilities",
            ),
            s(
                "Windows Media Player",
                r"Software\Clients\Media\WMP\Capabilities",
            ),
        ],
    );
}

fn open_with(system: &FakeSystem) {
    system.put(
        &hklm(r"SOFTWARE\Classes\.wdg\OpenWithProgids"),
        vec![binary("AcmeWidget.Document", Vec::new())],
    );
    system.put(
        &hkcu(&format!(r"{EXPLORER}\FileExts\.wdg\OpenWithProgids")),
        vec![binary("AcmeWidget.Document", Vec::new())],
    );
    system.put(
        &hkcu(&format!(r"{EXPLORER}\FileExts\.wdg\OpenWithList")),
        vec![
            s("a", "widget.exe"),
            s("b", "notepad.exe"),
            s("MRUList", "ab"),
        ],
    );
}
