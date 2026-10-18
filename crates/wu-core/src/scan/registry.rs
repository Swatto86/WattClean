//! Leftover keys in the software registry: the uninstall entry, keys named
//! after the program, and the publisher's key when nothing else is in it.

use super::{Confidence, Context, Found, Mode};
use crate::identity::Identity;
use crate::model::{Hive, RegPath};
use crate::system::{key_exists, subkeys_or_empty, values_or_empty};

pub(super) fn software_roots() -> [RegPath; 3] {
    [
        RegPath::new(Hive::LocalMachine, "SOFTWARE"),
        RegPath::new(Hive::LocalMachine, r"SOFTWARE\WOW6432Node"),
        RegPath::new(Hive::CurrentUser, "Software"),
    ]
}

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    let system = context.system;
    if let Some(key) = &identity.uninstall_key
        && key_exists(system, key)
    {
        found.key(
            context,
            key.clone(),
            "Uninstall entry left behind",
            Confidence::High,
        );
    }
    for root in software_roots() {
        scan_software_root(identity, mode, context, found, &root, &[]);
    }
}

/// Keys named after the program directly under a Software root, and the
/// publisher's key; `foreign` names (folded) belong to programs registered
/// only in that root's account and are skipped.
pub(super) fn scan_software_root(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    root: &RegPath,
    foreign: &[String],
) {
    let theirs = |name: &str| {
        context.belongs_to_other(name) || foreign.contains(&crate::identity::name_key(name))
    };
    for name in subkeys_or_empty(context.system, root) {
        let key = root.child(&name);
        if identity.matches(&name, mode.name_match()) && !theirs(&name) {
            found.key(
                context,
                key,
                "Key named after the program",
                confidence(identity, &name),
            );
        } else if identity.is_publisher(&name) {
            publisher_key(identity, mode, context, found, &key);
        }
    }
}

fn confidence(identity: &Identity, name: &str) -> Confidence {
    if identity.matches_inner(name, crate::identity::Match::Exact) {
        Confidence::High
    } else {
        Confidence::Medium
    }
}

/// The program's key inside its publisher's, and the publisher's key itself
/// when it holds nothing else: either only this program, or nothing at all
/// because the uninstaller took the program's key and left the shell.
fn publisher_key(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    publisher: &RegPath,
) {
    let children = subkeys_or_empty(context.system, publisher);
    let mut matched = 0;
    for name in &children {
        if identity.matches_inner(name, mode.name_match()) && !context.belongs_to_other(name) {
            found.key(
                context,
                publisher.child(name),
                "Program key under the publisher's key",
                confidence(identity, name),
            );
            matched += 1;
        }
    }
    if context.publisher_shared(identity) || !values_or_empty(context.system, publisher).is_empty()
    {
        return;
    }
    if children.is_empty() {
        found.key(
            context,
            publisher.clone(),
            "Empty publisher key",
            Confidence::Medium,
        );
    } else if matched == children.len() {
        found.key(
            context,
            publisher.clone(),
            "Publisher key holding only this program",
            Confidence::Medium,
        );
    }
}
