//! Machine-wide records: shared DLL counters, Windows Installer folders,
//! firewall rules and services; plus empty software keys and the person's
//! include list.

use std::path::Path;

use super::{Category, Context, Found};
use crate::check::Verdict;
use crate::model::{Hive, RegPath};
use crate::paths::{command_target, is_within};
use crate::system::{subkeys_or_empty, text_value, values_or_empty};

const SHARED_DLLS: &str = r"Microsoft\Windows\CurrentVersion\SharedDLLs";

/// Shared DLL counters: for missing files, or (`unused`) already at zero
/// for files that still exist. Unused ones start unticked.
pub fn shared_dlls(context: &Context, out: &mut Vec<Found>, unused: bool) {
    for (software, view32) in [
        (RegPath::new(Hive::LocalMachine, "SOFTWARE"), false),
        (
            RegPath::new(Hive::LocalMachine, r"SOFTWARE\WOW6432Node"),
            true,
        ),
    ] {
        let key = software.child(SHARED_DLLS);
        for value in values_or_empty(context.system, &key) {
            let verdict = context.check.path(&value.name, view32);
            if !unused && verdict == Verdict::Missing {
                out.push(Found::value(
                    Category::MissingSharedDlls,
                    key.clone(),
                    &value.name,
                    format!("Counts uses of {}, which no longer exists.", value.name),
                ));
            } else if unused && verdict == Verdict::Present && value.as_u32() == Some(0) {
                out.push(
                    Found::value(
                        Category::UnusedSharedDlls,
                        key.clone(),
                        &value.name,
                        format!("No installed program claims {} any more.", value.name),
                    )
                    .unticked(),
                );
            }
        }
    }
}

/// Folders Windows Installer recorded that no longer exist.
pub fn msi_folders(context: &Context, out: &mut Vec<Found>) {
    let key = RegPath::new(
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\Folders",
    );
    for value in values_or_empty(context.system, &key) {
        if context.check.missing(&value.name, false) {
            out.push(Found::value(
                Category::MsiFolders,
                key.clone(),
                &value.name,
                format!("Folder {} no longer exists.", value.name),
            ));
        }
    }
}

/// Firewall rules whose program no longer exists.
pub fn firewall(context: &Context, out: &mut Vec<Found>) {
    let key = RegPath::new(
        Hive::LocalMachine,
        r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules",
    );
    for value in values_or_empty(context.system, &key) {
        let Some(rule) = value.as_text() else {
            continue;
        };
        let field = |name: &str| {
            rule.split('|').find_map(|part| {
                let (field, text) = part.split_once('=')?;
                field.eq_ignore_ascii_case(name).then(|| text.to_owned())
            })
        };
        let Some(app) = field("App") else { continue };
        if context.check.missing(&app, false) {
            let name = field("Name").unwrap_or_else(|| value.name.clone());
            out.push(Found::value(
                Category::Firewall,
                key.clone(),
                &value.name,
                format!("Rule \"{name}\" is for {app}, which no longer exists."),
            ));
        }
    }
}

/// Windows services (not drivers) whose program is missing. Never a service
/// inside the Windows folder, whatever the settings say. Unticked.
pub fn services(context: &Context, out: &mut Vec<Found>) {
    let services = RegPath::new(Hive::LocalMachine, r"SYSTEM\CurrentControlSet\Services");
    let windows = context.system.env_var("SystemRoot");
    for name in subkeys_or_empty(context.system, &services) {
        let key = services.child(&name);
        let values = values_or_empty(context.system, &key);
        let kind = values
            .iter()
            .find(|value| value.name.eq_ignore_ascii_case("Type"))
            .and_then(|value| value.as_u32());
        // Own-process and shared-process services, optionally interactive;
        // per-account service templates (0x50, 0x60) and drivers are left.
        if !matches!(kind, Some(0x10 | 0x20 | 0x110 | 0x120)) {
            continue;
        }
        let Some(target) = text_value(&values, "ImagePath").and_then(|text| command_target(&text))
        else {
            continue;
        };
        let expanded = crate::paths::expand(context.system, &target);
        if windows
            .as_deref()
            .is_some_and(|windows| is_within(Path::new(&expanded), Path::new(windows)))
        {
            continue;
        }
        if context.check.missing(&target, false) {
            let display = text_value(&values, "DisplayName").unwrap_or_else(|| name.clone());
            out.push(
                Found::key(
                    Category::Services,
                    key,
                    format!("Service \"{display}\" runs {target}, which no longer exists."),
                )
                .unticked(),
            );
        }
    }
}

/// `Software` children that are Windows' own structure, never a vendor.
const NOT_VENDORS: [&str; 12] = [
    "AppDataLow",
    "Classes",
    "Clients",
    "DefaultUserEnvironment",
    "Microsoft",
    "ODBC",
    "Partner",
    "Policies",
    "RegisteredApplications",
    "Setup",
    "Windows",
    "WOW6432Node",
];

/// Vendor keys, and product keys one level down, that hold no values and
/// no subkeys. Unticked: some programs use an empty key as a marker.
pub fn empty_keys(context: &Context, out: &mut Vec<Found>) {
    for (software, _) in context.software_roots() {
        for vendor in subkeys_or_empty(context.system, &software) {
            if NOT_VENDORS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&vendor))
            {
                continue;
            }
            let vendor_key = software.child(&vendor);
            let products = subkeys_or_empty(context.system, &vendor_key);
            if products.is_empty() {
                if is_empty(context, &vendor_key) {
                    out.push(
                        Found::key(
                            Category::EmptyKeys,
                            vendor_key,
                            format!("\"{vendor}\" holds nothing."),
                        )
                        .unticked(),
                    );
                }
                continue;
            }
            for product in products {
                let key = vendor_key.child(&product);
                if subkeys_or_empty(context.system, &key).is_empty() && is_empty(context, &key) {
                    out.push(
                        Found::key(
                            Category::EmptyKeys,
                            key,
                            format!("\"{vendor}\\{product}\" holds nothing."),
                        )
                        .unticked(),
                    );
                }
            }
        }
    }
}

/// No values, and the key could be read (an unreadable key is not empty).
fn is_empty(context: &Context, key: &RegPath) -> bool {
    context
        .system
        .values(key)
        .is_ok_and(|values| values.is_empty())
}

/// The contents of each include-list key: its values, and its subkeys when
/// the entry says so.
pub fn include_list(context: &Context, out: &mut Vec<Found>) {
    for (key, subkeys) in &context.options.includes {
        for value in values_or_empty(context.system, key) {
            out.push(Found::value(
                Category::IncludeList,
                key.clone(),
                &value.name,
                "In your include list.".into(),
            ));
        }
        if *subkeys {
            for name in subkeys_or_empty(context.system, key) {
                out.push(Found::key(
                    Category::IncludeList,
                    key.child(&name),
                    "In your include list, with everything beneath it.".into(),
                ));
            }
        }
    }
}
