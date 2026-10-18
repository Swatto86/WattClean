//! Another account on the PC with the widget's traces in its own registry
//! (read from its profile's hives), and a bundle installer whose uninstall
//! leaves its Package Cache and dependency records behind.

use std::io;
use std::path::Path;

use super::super::{Effect, FakeSystem};
use super::{UNINSTALL, file, hklm, s};
use crate::model::{RawValue, RegPath};

pub const BUNDLE_ID: &str = "{B0B0B0B0-0000-0000-0000-00000000B0B0}";
pub const BUNDLE_PACKAGE: &str = "{B1B1B1B1-0000-0000-0000-00000000B1B1}";
const DEPENDENCIES: &str = r"SOFTWARE\Classes\Installer\Dependencies";

pub(super) fn seed_other_account(system: &FakeSystem, root: &Path, exe: &str) -> io::Result<()> {
    file(&root.join(r"Users\other\NTUSER.DAT"), 256)?;
    file(
        &root.join(r"Users\other\AppData\Local\Microsoft\Windows\UsrClass.dat"),
        256,
    )?;
    let hive = RegPath::account_root("other");
    let classes = RegPath::account_classes_root("other");
    system.put(
        &hive.child(r"Software\Acme\Widget"),
        vec![s("LastUser", "other")],
    );
    system.put(
        &hive.child(r"Software\Microsoft\Windows\CurrentVersion\Run"),
        vec![
            s("AcmeWidget", &format!("\"{exe}\" --tray")),
            s("OtherUserTool", r"C:\Tools\out.exe"),
        ],
    );
    system.put(
        &hive.child(&format!(r"{UNINSTALL}\AcmeWidgetUser")),
        vec![
            s("DisplayName", "Acme Widget 2.1 (x64)"),
            s("UninstallString", &format!("\"{exe}\" /uninstall")),
        ],
    );
    // A program that only this account has: its keys stay.
    system.put(
        &hive.child(&format!(r"{UNINSTALL}\OtherUserTool")),
        vec![
            s("DisplayName", "Other User Tool"),
            s("UninstallString", r"C:\Tools\uninstall.exe"),
        ],
    );
    system.put(&hive.child(r"Software\OtherUserTool"), vec![s("A", "1")]);
    system.put(
        &classes.child(r"CLSID\{11111111-2222-3333-4444-555555555555}\InprocServer32"),
        vec![s("", &exe.replace("widget.exe", r"plugins\core.dll"))],
    );
    system.put(
        &classes.child(r"AcmeWidget.Document\shell\open\command"),
        vec![s("", &format!("\"{exe}\" \"%1\""))],
    );
    Ok(())
}

/// Woodgrove Runtime: a bundle whose uninstaller removes only its entry.
pub(super) fn seed_bundle(system: &FakeSystem, root: &Path) -> io::Result<()> {
    let cache = root.join(r"ProgramData\Package Cache");
    let setup = cache.join(BUNDLE_ID).join("WoodgroveSetup.exe");
    file(&setup, 2048)?;
    file(
        &cache
            .join(format!("{BUNDLE_PACKAGE}v1.0.0"))
            .join("runtime.msi"),
        2048,
    )?;
    file(
        &cache
            .join("{C0C0C0C0-0000-0000-0000-00000000C0C0}")
            .join("Other.exe"),
        64,
    )?;
    let key = hklm(&format!(r"{UNINSTALL}\{BUNDLE_ID}"));
    system.put(
        &key,
        vec![
            s("DisplayName", "Woodgrove Runtime"),
            s("DisplayVersion", "1.0.0"),
            s("Publisher", "Woodgrove Bank"),
            s("BundleCachePath", &setup.display().to_string()),
            s("BundleProviderKey", "WoodgroveRuntime"),
            s(
                "UninstallString",
                &format!("\"{}\" /uninstall", setup.display()),
            ),
            RawValue::dword("SystemComponent", 0),
        ],
    );
    let dependencies = hklm(DEPENDENCIES);
    system.put(
        &dependencies.child("WoodgroveRuntime"),
        vec![s("DisplayName", "Woodgrove Runtime"), s("Version", "1.0.0")],
    );
    system.put(
        &dependencies
            .child(BUNDLE_PACKAGE)
            .child("Dependents")
            .child(BUNDLE_ID),
        Vec::new(),
    );
    system.put(
        &dependencies.child(BUNDLE_PACKAGE),
        vec![
            s("DisplayName", "Woodgrove Runtime Core"),
            s("Version", "1.0.0"),
        ],
    );
    // Another bundle depending on the same package keeps that package's cache.
    system.put(
        &dependencies
            .child("{C0C0C0C0-0000-0000-0000-00000000C0C0}")
            .child("Dependents")
            .child("{C0C0C0C0-0000-0000-0000-00000000C0C0}"),
        Vec::new(),
    );
    system.add_effect(Effect {
        command_contains: "woodgrovesetup.exe".into(),
        delete_keys: vec![key],
        delete_paths: Vec::new(),
    });
    Ok(())
}
