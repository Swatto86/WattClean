//! Tailspin Paint: not installed yet. Its setup in Downloads is the one the
//! traced-install tests run. It creates a data folder whose name has nothing
//! to do with the program (only its install log finds it), adds a Run entry,
//! edits the existing PATH variable (recorded, never removed), and makes and
//! removes a temp file and a Restart Manager key on the way.

use std::io;
use std::path::Path;

use super::super::{Effect, FakeSystem, InstallEffect};
use super::{UNINSTALL, file, hkcu, hklm, s};
use crate::model::RawValue;

pub const SETUP: &str = r"Users\tester\Downloads\TailspinPaintSetup.exe";
const ENVIRONMENT: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

pub(super) fn seed_tailspin(system: &FakeSystem, root: &Path) -> io::Result<()> {
    file(&root.join(SETUP), 2048)?;
    system.put(
        &hklm(ENVIRONMENT),
        vec![RawValue::expand_string("Path", r"C:\Windows\system32")],
    );
    let dir = root.join(r"Program Files\Tailspin\Paint");
    let exe = dir.join("paint.exe");
    let uninstaller = dir.join("uninst.exe");
    let shortcut = root.join(r"Users\Public\Desktop\Tailspin Paint.lnk");
    let key = hklm(&format!(r"{UNINSTALL}\TailspinPaint"));
    system.add_install(InstallEffect {
        command_contains: "tailspinpaintsetup.exe".into(),
        folders: vec![
            root.join(r"Program Files\Tailspin"),
            dir.clone(),
            root.join(r"ProgramData\TSXData"),
        ],
        files: vec![
            exe.clone(),
            uninstaller.clone(),
            root.join(r"ProgramData\TSXData\brushes.db"),
            shortcut.clone(),
        ],
        keys: vec![
            (
                key.clone(),
                vec![
                    s("DisplayName", "Tailspin Paint"),
                    s("DisplayVersion", "1.0"),
                    s("Publisher", "Tailspin Toys"),
                    s("InstallLocation", &dir.display().to_string()),
                    s("UninstallString", &format!("\"{}\"", uninstaller.display())),
                ],
            ),
            (hklm(r"SOFTWARE\Tailspin"), Vec::new()),
            (hklm(r"SOFTWARE\Tailspin\Paint"), vec![s("Version", "1.0")]),
            (hkcu(r"Software\TSX"), Vec::new()),
            (hkcu(r"Software\TSX\Prefs"), vec![s("Theme", "light")]),
        ],
        new_values: vec![(
            hkcu(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            s("TSXUpdater", &format!("\"{}\" --update", exe.display())),
        )],
        changed_values: vec![(
            hklm(ENVIRONMENT),
            RawValue::expand_string("Path", &format!(r"C:\Windows\system32;{}", dir.display())),
        )],
        transient_file: Some(root.join(r"Users\tester\AppData\Local\Temp\tsx_setup.tmp")),
        transient_key: Some(hkcu(r"Software\Microsoft\RestartManager\Session0000")),
    });
    // Its uninstaller removes its entry and program files, and nothing else.
    system.add_effect(Effect {
        command_contains: r"tailspin\paint\uninst.exe".into(),
        delete_keys: vec![key],
        delete_paths: vec![exe, uninstaller, shortcut],
    });
    Ok(())
}
