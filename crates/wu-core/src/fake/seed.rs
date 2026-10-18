//! The fake machine's contents: Windows' folder registry, five programs with
//! their files, keys and the leftovers their uninstallers leave behind.

use std::io;
use std::path::Path;

pub mod accounts;
mod acme;
pub mod tailspin;
pub mod windows;

use super::{Effect, FakeSystem};
use crate::model::{Hive, RawValue, RegPath};
use crate::scan::shortcut::link_bytes;
use acme::{seed_helper, seed_widget};
use tailspin::seed_tailspin;

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
pub const VIEWER_CODE: &str = "{0A1B2C3D-1111-2222-3333-444455556666}";
pub const VIEWER_PACKED: &str = "D3C2B1A0111122223333444455556666";
/// Another MSI product sharing a component with the viewer.
const OTHER_PACKED: &str = "FFFFFFFF0000000000000000000000FF";

fn hklm(path: &str) -> RegPath {
    RegPath::new(Hive::LocalMachine, path)
}

fn hkcu(path: &str) -> RegPath {
    RegPath::new(Hive::CurrentUser, path)
}

fn s(name: &str, text: &str) -> RawValue {
    RawValue::string(name, text)
}

fn file(path: &Path, size: usize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, vec![b'x'; size])
}

pub(super) fn seed(system: &FakeSystem) -> io::Result<()> {
    let root = system.root().to_path_buf();
    let r = |rel: &str| root.join(rel);
    let text = |rel: &str| root.join(rel).display().to_string();

    for (name, rel) in [
        ("SystemRoot", "Windows"),
        ("ProgramFiles", "Program Files"),
        ("ProgramFiles(x86)", "Program Files (x86)"),
        ("ProgramData", "ProgramData"),
        ("USERPROFILE", r"Users\tester"),
        ("APPDATA", r"Users\tester\AppData\Roaming"),
        ("LOCALAPPDATA", r"Users\tester\AppData\Local"),
        ("TEMP", r"Users\tester\AppData\Local\Temp"),
        ("PUBLIC", r"Users\Public"),
    ] {
        system.set_env(name, &text(rel));
    }
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
        vec![s("SystemRoot", &text("Windows"))],
    );
    system.put(
        &hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion"),
        vec![
            s("ProgramFilesDir", &text("Program Files")),
            s("ProgramFilesDir (x86)", &text("Program Files (x86)")),
            s("CommonFilesDir", &text(r"Program Files\Common Files")),
            s(
                "CommonFilesDir (x86)",
                &text(r"Program Files (x86)\Common Files"),
            ),
        ],
    );
    let shell = r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders";
    system.put(
        &hklm(shell),
        vec![
            RawValue::expand_string("Common AppData", "%ProgramData%"),
            RawValue::expand_string("Common Desktop", r"%PUBLIC%\Desktop"),
            RawValue::expand_string(
                "Common Programs",
                r"%ProgramData%\Microsoft\Windows\Start Menu\Programs",
            ),
        ],
    );
    system.put(
        &hkcu(shell),
        vec![
            RawValue::expand_string("AppData", r"%USERPROFILE%\AppData\Roaming"),
            RawValue::expand_string("Local AppData", r"%USERPROFILE%\AppData\Local"),
            RawValue::expand_string("Desktop", r"%USERPROFILE%\OneDrive\Desktop"),
            RawValue::expand_string(
                "Programs",
                r"%APPDATA%\Microsoft\Windows\Start Menu\Programs",
            ),
            RawValue::expand_string("Personal", r"%USERPROFILE%\Documents"),
        ],
    );
    file(&r(r"Windows\System32\msiexec.exe"), 16)?;
    file(&r(r"Users\tester\Documents\notes.txt"), 16)?;

    seed_widget(system, &root)?;
    seed_helper(system, &root)?;
    seed_notes(system, &root)?;
    seed_fabrikam(system, &root)?;
    seed_broken(system, &root)?;
    seed_tailspin(system, &root)?;
    accounts::seed_bundle(system, &root)?;
    system.put(
        &hklm(&format!(r"{UNINSTALL}\KB5034441")),
        vec![
            s("DisplayName", "Security Update for Windows (KB5034441)"),
            s("ReleaseType", "Security Update"),
            s("UninstallString", "wusa.exe /uninstall /kb:5034441"),
        ],
    );
    Ok(())
}

fn seed_notes(system: &FakeSystem, root: &Path) -> io::Result<()> {
    let dir = root.join(r"Users\tester\AppData\Local\Programs\Contoso Notes");
    let uninstaller = dir
        .join("Uninstall Contoso Notes.exe")
        .display()
        .to_string();
    file(&dir.join("Contoso Notes.exe"), 8192)?;
    file(&dir.join("Uninstall Contoso Notes.exe"), 1024)?;
    file(
        &root.join(r"Users\tester\AppData\Roaming\Contoso Notes\Local Storage\leveldb.log"),
        700,
    )?;
    file(
        &root.join(
            r"Users\tester\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Contoso Notes.lnk",
        ),
        64,
    )?;
    let key = hkcu(&format!(r"{UNINSTALL}\ContosoNotes"));
    system.put(
        &key,
        vec![
            s("DisplayName", "Contoso Notes"),
            s("DisplayVersion", "5.0.2"),
            s("Publisher", "Contoso"),
            s("InstallLocation", &dir.display().to_string()),
            s(
                "UninstallString",
                &format!("\"{uninstaller}\" /currentuser"),
            ),
            s(
                "QuietUninstallString",
                &format!("\"{uninstaller}\" /currentuser /S"),
            ),
        ],
    );
    system.put(&hkcu(r"Software\ContosoNotes"), vec![s("Theme", "dark")]);
    // Its uninstaller takes its key and data folder out of the publisher's,
    // leaving both shells empty.
    system.put(&hkcu(r"Software\Contoso\Notes"), vec![s("Window", "1,1")]);
    file(
        &root.join(r"Users\tester\AppData\Local\Contoso\Notes\cache.db"),
        50,
    )?;
    let menu =
        root.join(r"Users\tester\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Contoso");
    std::fs::create_dir_all(menu.join("Tools"))?;
    let exe = dir.join("Contoso Notes.exe").display().to_string();
    std::fs::write(menu.join("Contoso Notes Manual.lnk"), link_bytes(&exe))?;
    std::fs::write(menu.join(r"Tools\Notes Tools.lnk"), link_bytes(&exe))?;
    // A service from an older version, pointing at a file long gone.
    system.put(
        &hklm(r"SYSTEM\CurrentControlSet\Services\ContosoNotes"),
        vec![RawValue::expand_string("ImagePath", r"D:\old\notes.exe")],
    );
    system.add_effect(Effect {
        command_contains: "uninstall contoso notes.exe".into(),
        delete_keys: vec![key, hkcu(r"Software\Contoso\Notes")],
        delete_paths: vec![dir, root.join(r"Users\tester\AppData\Local\Contoso\Notes")],
    });
    Ok(())
}

fn seed_fabrikam(system: &FakeSystem, root: &Path) -> io::Result<()> {
    let viewer = root.join(r"Program Files (x86)\Fabrikam\Viewer");
    file(&viewer.join("viewer.exe"), 4096)?;
    file(
        &root.join(r"Users\tester\AppData\Roaming\Fabrikam\Viewer\recent.xml"),
        200,
    )?;
    let key = hklm(&format!(r"{UNINSTALL_32}\{VIEWER_CODE}"));
    system.put(
        &key,
        vec![
            s("DisplayName", "Fabrikam Viewer"),
            s("DisplayVersion", "3.4.1"),
            s("Publisher", "Fabrikam, Inc."),
            s("InstallLocation", &viewer.display().to_string()),
            s("UninstallString", &format!("MsiExec.exe /I{VIEWER_CODE}")),
            RawValue::dword("WindowsInstaller", 1),
        ],
    );
    let products = hklm(&format!(
        r"SOFTWARE\Classes\Installer\Products\{VIEWER_PACKED}"
    ));
    system.put(
        &products,
        vec![
            s("ProductName", "Fabrikam Viewer"),
            s(
                "ProductIcon",
                &viewer.join("viewer.exe").display().to_string(),
            ),
        ],
    );
    // Windows Installer's component records: the viewer's own file, a shared
    // file it put under Common Files, and another product's beside them.
    let shared = root.join(r"Program Files (x86)\Common Files\Fabrikam\Shared\fvcore.dll");
    file(&shared, 512)?;
    let components = hklm(crate::msi::COMPONENTS);
    system.put(
        &components.child("{C1C1C1C1-0000-0000-0000-000000000001}"),
        vec![s(
            VIEWER_PACKED,
            &viewer.join("viewer.exe").display().to_string(),
        )],
    );
    system.put(
        &components.child("{C2C2C2C2-0000-0000-0000-000000000002}"),
        vec![
            s(VIEWER_PACKED, &shared.display().to_string()),
            s(OTHER_PACKED, &shared.display().to_string()),
        ],
    );
    system.put(
        &components.child("{C3C3C3C3-0000-0000-0000-000000000003}"),
        vec![s(
            OTHER_PACKED,
            &root.join(r"Windows\System32\foo.dll").display().to_string(),
        )],
    );
    system.put(
        &hklm(crate::msi::UPGRADE_CODES).child("{U1U1U1U1-0000-0000-0000-000000000001}"),
        vec![s(VIEWER_PACKED, ""), s(OTHER_PACKED, "")],
    );
    system.put(
        &hklm(&format!(
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Management\ARPCache\{VIEWER_CODE}"
        )),
        vec![RawValue::dword("SlowInfoCache", 1)],
    );
    let user_data = hklm(&format!(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Products\{VIEWER_PACKED}\InstallProperties"
    ));
    system.put(&user_data, vec![s("DisplayName", "Fabrikam Viewer")]);
    system.put(
        &hkcu(r"Software\Fabrikam\Viewer"),
        vec![s("LastFile", "a.fv")],
    );
    system.add_effect(Effect {
        command_contains: VIEWER_CODE.to_lowercase(),
        delete_keys: vec![key, products],
        delete_paths: vec![viewer],
    });

    let runtime = root.join(r"Program Files\Fabrikam\Runtime");
    file(&runtime.join("fabrt.dll"), 1024)?;
    system.put(
        &hklm(&format!(r"{UNINSTALL}\FabrikamRuntime")),
        vec![
            s("DisplayName", "Fabrikam Runtime Core"),
            s("Publisher", "Fabrikam, Inc."),
            s("InstallLocation", &runtime.display().to_string()),
            s(
                "UninstallString",
                &format!("\"{}\"", runtime.join("remove.exe").display()),
            ),
            RawValue::dword("SystemComponent", 1),
        ],
    );
    Ok(())
}

fn seed_broken(system: &FakeSystem, root: &Path) -> io::Result<()> {
    let dir = root.join(r"Program Files\Northwind\Broken Tool");
    file(&dir.join("tool.exe"), 2048)?;
    system.put(
        &hklm(&format!(r"{UNINSTALL}\BrokenTool")),
        vec![
            s("DisplayName", "Northwind Broken Tool"),
            s("Publisher", "Northwind Traders"),
            s("InstallLocation", &dir.display().to_string()),
            // Its uninstaller was deleted by hand: running it fails.
            s(
                "UninstallString",
                &format!("\"{}\"", dir.join("uninst.exe").display()),
            ),
        ],
    );
    system.put(
        &hkcu(r"Software\Northwind\Broken Tool"),
        vec![s("Window", "100,100")],
    );
    Ok(())
}
