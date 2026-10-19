//! Entries that start or describe a program: startup items, App Paths,
//! application registrations and uninstall entries.

use std::path::Path;

use super::{Category, Context, Found};
use crate::check::Verdict;
use crate::model::{Hive, RawValue, RegPath};
use crate::paths::command_target;
use crate::system::{subkeys_or_empty, text_value, values_or_empty};

const RUN_KEYS: [&str; 5] = [
    r"Microsoft\Windows\CurrentVersion\Run",
    r"Microsoft\Windows\CurrentVersion\RunOnce",
    r"Microsoft\Windows\CurrentVersion\RunServices",
    r"Microsoft\Windows\CurrentVersion\RunServicesOnce",
    r"Microsoft\Windows\CurrentVersion\Policies\Explorer\Run",
];
const APPROVED: &str = r"Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

/// Run entries whose program is missing, and Task Manager's on/off switches
/// left for startup items that no longer exist anywhere.
pub fn startup(context: &Context, out: &mut Vec<Found>) {
    let mut run_names = Vec::new();
    for (software, view32) in context.software_roots() {
        for run in RUN_KEYS {
            let key = software.child(run);
            for value in values_or_empty(context.system, &key) {
                run_names.push(value.name.to_lowercase());
                let Some(target) = value.as_text().and_then(|text| command_target(&text)) else {
                    continue;
                };
                if context.check_for(&key).missing(&target, view32) {
                    out.push(Found::value(
                        Category::Startup,
                        key.clone(),
                        &value.name,
                        format!("Starts {target}, which no longer exists."),
                    ));
                }
            }
        }
    }
    let mut approved = vec![RegPath::new(
        Hive::LocalMachine,
        &format!(r"SOFTWARE\{APPROVED}"),
    )];
    approved.extend(
        context
            .users
            .iter()
            .map(|user| user.software.child(APPROVED)),
    );
    for key in approved {
        for list in ["Run", "Run32"] {
            let list_key = key.child(list);
            for value in values_or_empty(context.system, &list_key) {
                if switched_on(&value) && !run_names.contains(&value.name.to_lowercase()) {
                    out.push(Found::value(
                        Category::Startup,
                        list_key.clone(),
                        &value.name,
                        format!(
                            "Task Manager's startup switch for \"{}\", which is no longer set to start.",
                            value.name
                        ),
                    ));
                }
            }
        }
        startup_folder_switches(context, &key, out);
    }
}

/// Task Manager's switch reads "on" (`02` or `06` first). A switch set to
/// "off" records the person's choice, which would be lost if the item came
/// back, so only "on" switches for removed items are offered.
fn switched_on(value: &RawValue) -> bool {
    matches!(value.bytes.first(), Some(0x02 | 0x06))
}

/// Switches for shortcuts that are no longer in either Startup folder.
fn startup_folder_switches(context: &Context, approved: &RegPath, out: &mut Vec<Found>) {
    // Other accounts' Startup folders are not known here, so only this
    // account's switches and the machine's are judged.
    if approved.hive == Hive::Users {
        return;
    }
    // The account's own Startup folder and the one shared by all accounts.
    let folders: Vec<String> = ["APPDATA", "ProgramData"]
        .into_iter()
        .filter_map(|variable| {
            let base = context.system.env_var(variable)?;
            Some(format!(
                r"{base}\Microsoft\Windows\Start Menu\Programs\Startup"
            ))
        })
        .collect();
    if folders.len() < 2 || !folders.iter().all(|folder| Path::new(folder).is_dir()) {
        return;
    }
    let key = approved.child("StartupFolder");
    for value in values_or_empty(context.system, &key) {
        let present = folders
            .iter()
            .any(|folder| Path::new(folder).join(&value.name).exists());
        if switched_on(&value) && !present {
            out.push(Found::value(
                Category::Startup,
                key.clone(),
                &value.name,
                format!("Task Manager's startup switch for \"{}\", which is no longer in a Startup folder.", value.name),
            ));
        }
    }
}

/// App Paths entries and `Classes\Applications` registrations whose
/// program is missing.
pub fn app_paths(context: &Context, out: &mut Vec<Found>) {
    for (software, view32) in context.software_roots() {
        let app_paths = software.child(r"Microsoft\Windows\CurrentVersion\App Paths");
        for name in subkeys_or_empty(context.system, &app_paths) {
            let key = app_paths.child(&name);
            let values = values_or_empty(context.system, &key);
            let Some(target) = text_value(&values, "").and_then(|text| command_target(&text))
            else {
                continue;
            };
            if context.check_for(&key).missing(&target, view32) {
                out.push(Found::key(
                    Category::AppPaths,
                    key,
                    format!(
                        "Run-dialog shortcut \"{name}\" opens {target}, which no longer exists."
                    ),
                ));
            }
        }
    }
    for classes in context.classes_roots() {
        let applications = classes.child("Applications");
        for name in subkeys_or_empty(context.system, &applications) {
            let key = applications.child(&name);
            if let Some(target) = all_commands_missing(context, &key) {
                out.push(Found::key(
                    Category::AppPaths,
                    key,
                    format!(
                        "Application registration \"{name}\" runs {target}, which no longer exists."
                    ),
                ));
            }
        }
    }
}

/// The first program named by `key\shell\*\command` when there is at least
/// one command and every one of them names a missing file; `None` when any
/// command works, cannot be judged, or is handled by a COM object instead.
pub(super) fn all_commands_missing(context: &Context, key: &RegPath) -> Option<String> {
    let shell = key.child("shell");
    let mut first = None;
    for verb in subkeys_or_empty(context.system, &shell) {
        let command = shell.child(&verb).child("command");
        let values = values_or_empty(context.system, &command);
        if values
            .iter()
            .any(|value| value.name.eq_ignore_ascii_case("DelegateExecute"))
        {
            return None;
        }
        let target = text_value(&values, "").and_then(|text| command_target(&text))?;
        if context.check_for(key).path(&target, false) != Verdict::Missing {
            return None;
        }
        first.get_or_insert(target);
    }
    first
}

/// Uninstall entries whose uninstaller is missing and whose install folder
/// and icon, when it names them, are gone too. Windows Installer's own
/// entries are left to Windows Installer, and setup bundles' to their
/// packages: a bundle's cached setup can be cleaned away while what it
/// installed stays. An entry naming no install folder is offered unticked.
pub fn uninstall(context: &Context, out: &mut Vec<Found>) {
    for (software, view32) in context.software_roots() {
        let list = software.child(r"Microsoft\Windows\CurrentVersion\Uninstall");
        for id in subkeys_or_empty(context.system, &list) {
            let key = list.child(&id);
            let values = values_or_empty(context.system, &key);
            let msi = values.iter().any(|value| {
                value.name.eq_ignore_ascii_case("WindowsInstaller") && value.as_u32() == Some(1)
            });
            let bundle = values.iter().any(|value| {
                value.name.eq_ignore_ascii_case("BundleProviderKey")
                    || value.name.eq_ignore_ascii_case("BundleCachePath")
            });
            if msi || bundle {
                continue;
            }
            let Some(target) =
                text_value(&values, "UninstallString").and_then(|text| command_target(&text))
            else {
                continue;
            };
            let check = context.check_for(&key);
            if !check.missing(&target, view32) {
                continue;
            }
            let folder =
                text_value(&values, "InstallLocation").filter(|text| !text.trim().is_empty());
            let folder_present = folder
                .as_ref()
                .is_some_and(|folder| check.path(folder, view32) != Verdict::Missing);
            let icon_present = text_value(&values, "DisplayIcon")
                .and_then(|text| command_target(&text))
                .is_some_and(|icon| check.path(&icon, view32) != Verdict::Missing);
            if folder_present || icon_present {
                continue;
            }
            let name = text_value(&values, "DisplayName").unwrap_or_else(|| id.clone());
            let found = Found::key(
                Category::Uninstall,
                key,
                format!("\"{name}\": its uninstaller {target} no longer exists."),
            );
            out.push(if folder.is_some() {
                found
            } else {
                found.unticked()
            });
        }
    }
}
