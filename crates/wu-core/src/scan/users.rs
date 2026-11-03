//! Advanced: what the program left in other accounts on this PC, in their
//! data folders and in their own registry. An account's registry is read
//! from its profile's hive files (loaded for the scan when the account is
//! not signed in); a hive that cannot be read is reported, not guessed at.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::{Context, Found, Mode, Target};
use crate::identity::{Identity, Match};
use crate::model::RegPath;
use crate::paths::{embedded_path, expand, is_within};
use crate::programs::UNINSTALL;
use crate::system::{Account, subkeys_or_empty, text_value, values_or_empty};

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    if mode < Mode::Advanced {
        return;
    }
    for account in context.system.other_accounts() {
        if context.progress.cancelled() {
            return;
        }
        let uninstall = RegPath::account_root(&account.name).child(UNINSTALL);
        let live = live_installs(identity, context, &uninstall);
        let mut own = Found::default();
        for root in [r"AppData\Roaming", r"AppData\Local", r"AppData\LocalLow"] {
            super::files::named_children(
                identity,
                mode,
                context,
                &mut own,
                &account.profile.join(root),
            );
        }
        registry(identity, mode, context, &mut own, &account);
        for warning in own.warnings {
            found.warn(warning);
        }
        for (target, reason, confidence) in own.items {
            if !is_live(&target, &live) {
                found.add(target, reason, confidence);
            }
        }
    }
}

/// The account's own copies of this program that are still installed: the
/// uninstall entry and the folders its uninstaller or install location is in.
fn live_installs(
    identity: &Identity,
    context: &Context<'_>,
    uninstall: &RegPath,
) -> Vec<(RegPath, Vec<PathBuf>)> {
    let mut live = Vec::new();
    for name in subkeys_or_empty(context.system, uninstall) {
        let key = uninstall.child(&name);
        let values = values_or_empty(context.system, &key);
        let named = text_value(&values, "DisplayName")
            .is_some_and(|display| identity.matches(&display, Match::Exact));
        if !named {
            continue;
        }
        let mut dirs = Vec::new();
        let location = text_value(&values, "InstallLocation")
            .map(|text| PathBuf::from(expand(context.system, &text)));
        dirs.extend(location.filter(|dir| dir.is_dir()));
        for field in ["UninstallString", "QuietUninstallString", "DisplayIcon"] {
            let command = text_value(&values, field).unwrap_or_default();
            let file =
                embedded_path(&expand(context.system, &command)).filter(|file| file.is_file());
            dirs.extend(file.and_then(|file| file.parent().map(Path::to_path_buf)));
        }
        if !dirs.is_empty() {
            live.push((key, dirs));
        }
    }
    live
}

/// Is `target` the entry of a live install, or the folder it lives in?
fn is_live(target: &Target, live: &[(RegPath, Vec<PathBuf>)]) -> bool {
    match target {
        Target::Key { key } | Target::Value { key, .. } => live
            .iter()
            .any(|(entry, _)| key.folded() == entry.folded() || key.is_within(entry)),
        Target::Folder { path } | Target::File { path } => live
            .iter()
            .flat_map(|(_, dirs)| dirs)
            .any(|dir| is_within(dir, path) || is_within(path, dir)),
    }
}

fn registry(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    account: &Account,
) {
    let root = RegPath::account_root(&account.name);
    let software = root.child("Software");
    match context.system.subkeys(&software) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return,
        Err(error) => {
            let why = match error.kind() {
                ErrorKind::PermissionDenied => {
                    "it needs administrator rights, or the account is signing in".to_owned()
                }
                ErrorKind::InvalidData => {
                    "its registry file has unsaved changes that only signing in as that account applies"
                        .to_owned()
                }
                _ => format!("it could not be read: {error}"),
            };
            found.warn(format!(
                "The registry of the account {} was not scanned: {why}.",
                account.name
            ));
            return;
        }
    }
    // Programs registered only in that account are its own, not leftovers.
    let uninstall = root.child(UNINSTALL);
    let foreign: Vec<String> = subkeys_or_empty(context.system, &uninstall)
        .iter()
        .filter_map(|name| {
            text_value(
                &values_or_empty(context.system, &uninstall.child(name)),
                "DisplayName",
            )
        })
        .filter(|display| !identity.matches(display, Match::Exact))
        .map(|display| crate::identity::name_key(&display))
        .collect();
    super::registry::scan_software_root(identity, mode, context, found, &software, &foreign);
    super::clients::duplicate_entries_in(identity, context, found, &[uninstall]);
    let current = software.child(r"Microsoft\Windows\CurrentVersion");
    super::registrations::startup(identity, context, found, &current, "Run");
    super::registrations::app_paths(identity, context, found, &current.child("App Paths"));
    let classes = RegPath::account_classes_root(&account.name);
    if context.system.subkeys(&classes).is_ok() {
        super::classes::com_classes(identity, context, found, &classes.child("CLSID"));
        super::classes::file_types(identity, context, found, &classes);
    }
}
