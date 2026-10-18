//! Moderate and up: Windows' default-program registrations (`Clients`), the
//! application registrations that point at them, duplicate uninstall
//! entries of the same program, and Explorer's cache of uninstall entries.

use super::{Confidence, Context, Found, Mode};
use crate::identity::{Identity, Match};
use crate::model::{Hive, RegPath};
use crate::programs::uninstall_roots;
use crate::system::{key_exists, subkeys_or_empty, text_value, values_or_empty};

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    duplicate_entries(identity, context, found);
    if mode < Mode::Moderate {
        return;
    }
    for base in ["SOFTWARE", r"SOFTWARE\WOW6432Node"] {
        clients(
            identity,
            mode,
            context,
            found,
            &RegPath::new(Hive::LocalMachine, base),
        );
    }
    clients(
        identity,
        mode,
        context,
        found,
        &RegPath::new(Hive::CurrentUser, "Software"),
    );
    for key in [
        RegPath::new(Hive::LocalMachine, r"SOFTWARE\RegisteredApplications"),
        RegPath::new(
            Hive::LocalMachine,
            r"SOFTWARE\WOW6432Node\RegisteredApplications",
        ),
        RegPath::new(Hive::CurrentUser, r"Software\RegisteredApplications"),
    ] {
        registered_applications(context, found, &key);
    }
    arp_cache(identity, context, found);
}

/// `Clients\<category>\<client>` keys named after the program or opening it.
fn clients(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    base: &RegPath,
) {
    let clients = base.child("Clients");
    for category in subkeys_or_empty(context.system, &clients) {
        let category = clients.child(&category);
        for name in subkeys_or_empty(context.system, &category) {
            let key = category.child(&name);
            let command = text_value(
                &values_or_empty(context.system, &key.child(r"shell\open\command")),
                "",
            )
            .unwrap_or_default();
            let icon = text_value(
                &values_or_empty(context.system, &key.child("DefaultIcon")),
                "",
            )
            .unwrap_or_default();
            let named =
                |strength| identity.matches(&name, strength) && !context.belongs_to_other(&name);
            let level = if identity.references(&command)
                || identity.references(&icon)
                || named(Match::Exact)
            {
                Confidence::High
            } else if named(mode.name_match()) {
                Confidence::Medium
            } else {
                continue;
            };
            found.key(context, key, "Default-program registration", level);
        }
    }
}

/// Values pointing at a Capabilities key inside a key already found.
fn registered_applications(context: &Context<'_>, found: &mut Found, key: &RegPath) {
    let hive = key.hive;
    for value in values_or_empty(context.system, key) {
        let Some(data) = value.as_text() else {
            continue;
        };
        let capabilities = RegPath::new(hive, data.trim().trim_matches('\\'));
        let owner = capabilities
            .parent()
            .filter(|_| capabilities.name().eq_ignore_ascii_case("Capabilities"))
            .unwrap_or(capabilities);
        if found.covers_key(&owner) {
            found.value(
                context,
                key.clone(),
                value.name,
                "Application registration pointing at its key",
                Confidence::High,
            );
        }
    }
}

/// Other uninstall entries of the same program (another view, or a per-user
/// copy) whose name is exactly ours, and their Explorer cache records.
fn duplicate_entries(identity: &Identity, context: &Context<'_>, found: &mut Found) {
    let roots: Vec<RegPath> = uninstall_roots()
        .into_iter()
        .map(|(root, _)| root)
        .collect();
    duplicate_entries_in(identity, context, found, &roots);
}

pub(super) fn duplicate_entries_in(
    identity: &Identity,
    context: &Context<'_>,
    found: &mut Found,
    roots: &[RegPath],
) {
    let own = identity.uninstall_key.as_ref().map(RegPath::folded);
    for root in roots {
        for name in subkeys_or_empty(context.system, root) {
            let key = root.child(&name);
            if own.as_deref() == Some(key.folded().as_str()) {
                continue;
            }
            let display = text_value(&values_or_empty(context.system, &key), "DisplayName")
                .unwrap_or_default();
            if !display.is_empty() && identity.matches(&display, Match::Exact) {
                found.key(
                    context,
                    key,
                    "Another entry of the same program",
                    Confidence::High,
                );
            }
        }
    }
}

/// Explorer's cache of uninstall entries, keyed like the entries themselves.
fn arp_cache(identity: &Identity, context: &Context<'_>, found: &mut Found) {
    let Some(name) = identity.uninstall_key.as_ref().map(RegPath::name) else {
        return;
    };
    for base in ["SOFTWARE", r"SOFTWARE\WOW6432Node"] {
        let key = RegPath::new(
            Hive::LocalMachine,
            &format!(r"{base}\Microsoft\Windows\CurrentVersion\App Management\ARPCache\{name}"),
        );
        if key_exists(context.system, &key) {
            found.key(
                context,
                key,
                "Explorer's cached uninstall entry",
                Confidence::High,
            );
        }
    }
}
