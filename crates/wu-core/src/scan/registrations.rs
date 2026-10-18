//! Moderate and up: the places Windows records a program by the path of one
//! of its files: App Paths, startup entries, shared file counts, services,
//! firewall rules, fonts, Explorer's caches and compatibility records.

use std::path::Path;

use super::{Confidence, Context, Found, Mode};
use crate::identity::{Identity, Match};
use crate::model::{Hive, RegPath};
use crate::paths::{embedded_path, expand, is_within};
use crate::system::{subkeys_or_empty, text_value, values_or_empty};

const CURRENT: &str = r"Microsoft\Windows\CurrentVersion";

fn hklm(path: &str) -> RegPath {
    RegPath::new(Hive::LocalMachine, path)
}

fn hkcu(path: &str) -> RegPath {
    RegPath::new(Hive::CurrentUser, path)
}

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    if mode < Mode::Moderate || (identity.install_dirs.is_empty() && identity.exe_names.is_empty())
    {
        return;
    }
    for base in ["SOFTWARE", r"SOFTWARE\WOW6432Node"] {
        app_paths(
            identity,
            context,
            found,
            &hklm(&format!(r"{base}\{CURRENT}\App Paths")),
        );
        let approved = if base == "SOFTWARE" { "Run" } else { "Run32" };
        startup(
            identity,
            context,
            found,
            &hklm(&format!(r"{base}\{CURRENT}")),
            approved,
        );
        shared_dlls(
            identity,
            context,
            found,
            &hklm(&format!(r"{base}\{CURRENT}\SharedDLLs")),
        );
    }
    app_paths(
        identity,
        context,
        found,
        &hkcu(&format!(r"Software\{CURRENT}\App Paths")),
    );
    startup(
        identity,
        context,
        found,
        &hkcu(&format!(r"Software\{CURRENT}")),
        "Run",
    );
    services(identity, context, found);
    values_referencing(
        identity,
        context,
        found,
        &hklm(
            r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules",
        ),
        "Firewall rule for one of its files",
    );
    for key in [
        hklm(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"),
        hkcu(r"Software\Microsoft\Windows NT\CurrentVersion\Fonts"),
    ] {
        values_referencing(
            identity,
            context,
            found,
            &key,
            "Font registered from its files",
        );
    }
    for (key, reason) in [
        (
            hkcu(r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"),
            "Explorer's cached program name",
        ),
        (
            hklm(&format!(r"SOFTWARE\{CURRENT}\Installer\Folders")),
            "Windows Installer folder record",
        ),
        (
            hkcu(
                r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store",
            ),
            "Compatibility record",
        ),
        (
            hkcu(r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers"),
            "Compatibility setting",
        ),
        (
            hklm(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers"),
            "Compatibility setting",
        ),
    ] {
        value_names_within(identity, context, found, &key, reason);
    }
}

pub(super) fn app_paths(
    identity: &Identity,
    context: &Context<'_>,
    found: &mut Found,
    app_paths: &RegPath,
) {
    for name in subkeys_or_empty(context.system, app_paths) {
        let key = app_paths.child(&name);
        let target = text_value(&values_or_empty(context.system, &key), "").unwrap_or_default();
        let ours = identity.references(&target)
            || (identity.exe_names.contains(&name.to_lowercase())
                && embedded_path(&target).is_none_or(|path| !path.exists()));
        if ours {
            found.key(
                context,
                key,
                "Registered program path (App Paths)",
                Confidence::High,
            );
        }
    }
}

/// Run and RunOnce entries starting one of its files, with the matching
/// entry in Explorer's startup approval list.
pub(super) fn startup(
    identity: &Identity,
    context: &Context<'_>,
    found: &mut Found,
    base: &RegPath,
    approved: &str,
) {
    let approvals = base.child(&format!(r"Explorer\StartupApproved\{approved}"));
    for run in ["Run", "RunOnce"] {
        let key = base.child(run);
        for value in values_or_empty(context.system, &key) {
            let text = value.as_text().unwrap_or_default();
            if !identity.references(&expand(context.system, &text)) {
                continue;
            }
            let approval = values_or_empty(context.system, &approvals)
                .into_iter()
                .find(|known| known.name.eq_ignore_ascii_case(&value.name));
            found.value(
                context,
                key.clone(),
                value.name,
                "Starts with Windows",
                Confidence::High,
            );
            if let Some(approval) = approval {
                found.value(
                    context,
                    approvals.clone(),
                    approval.name,
                    "Startup approval of that entry",
                    Confidence::High,
                );
            }
        }
    }
}

fn shared_dlls(identity: &Identity, context: &Context<'_>, found: &mut Found, key: &RegPath) {
    for value in values_or_empty(context.system, key) {
        if identity.owns_path(Path::new(&value.name)) {
            found.value(
                context,
                key.clone(),
                value.name,
                "Shared file count for one of its files",
                Confidence::High,
            );
        }
    }
}

/// Services running one of its files, or named after it with their file gone.
fn services(identity: &Identity, context: &Context<'_>, found: &mut Found) {
    let services = hklm(r"SYSTEM\CurrentControlSet\Services");
    for name in subkeys_or_empty(context.system, &services) {
        let key = services.child(&name);
        let values = values_or_empty(context.system, &key);
        let image = expand(
            context.system,
            &text_value(&values, "ImagePath").unwrap_or_default(),
        );
        if identity.references(&image) {
            found.key(
                context,
                key,
                "Windows service running one of its files",
                Confidence::High,
            );
        } else if identity.matches_inner(&name, Match::Exact)
            && !context.belongs_to_other(&name)
            && embedded_path(&image).is_some_and(|file| !file.exists())
        {
            found.key(
                context,
                key,
                "Windows service named after the program, its file gone",
                Confidence::Medium,
            );
        }
    }
}

/// Values whose data points into the program's folders.
pub(super) fn values_referencing(
    identity: &Identity,
    context: &Context<'_>,
    found: &mut Found,
    key: &RegPath,
    reason: &str,
) {
    for value in values_or_empty(context.system, key) {
        let text = value.as_text().unwrap_or_default();
        if identity.references(&expand(context.system, &text)) {
            found.value(context, key.clone(), value.name, reason, Confidence::High);
        }
    }
}

/// Values named by a path inside the program's folders.
pub(super) fn value_names_within(
    identity: &Identity,
    context: &Context<'_>,
    found: &mut Found,
    key: &RegPath,
    reason: &str,
) {
    for value in values_or_empty(context.system, key) {
        let path = embedded_path(&value.name);
        if path.is_some_and(|path| {
            identity
                .install_dirs
                .iter()
                .any(|dir| is_within(&path, dir))
        }) {
            found.value(context, key.clone(), value.name, reason, Confidence::High);
        }
    }
}
