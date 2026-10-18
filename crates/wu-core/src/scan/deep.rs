//! Advanced mode: walk the software registry for keys carrying the
//! program's name and values pointing into its folders, and look one level
//! deeper in the data folders.

use super::files::{file_name, list};
use super::registry::software_roots;
use super::{Confidence, Context, Found, Mode};
use crate::identity::{Identity, Match};
use crate::model::RegPath;
use crate::paths::embedded_path;
use crate::system::subkeys_or_empty;

/// Subtrees that are huge, owned by Windows, or covered elsewhere.
const SKIP: &[&str] = &[
    "software\\classes",
    "software\\wow6432node\\classes",
    "software\\microsoft\\windows\\currentversion\\installer",
    "software\\microsoft\\windows\\currentversion\\component based servicing",
    "software\\microsoft\\windows\\currentversion\\winevt",
    "software\\microsoft\\windows\\currentversion\\appx",
    "software\\microsoft\\windows\\currentversion\\uninstall",
    "software\\wow6432node\\microsoft\\windows\\currentversion\\uninstall",
    "software\\microsoft\\windows nt\\currentversion\\perflib",
    "software\\microsoft\\windows defender",
    "software\\microsoft\\windows search",
];

const MAX_DEPTH: usize = 8;
/// Bounds the walk on a machine with an unusually large registry.
const MAX_KEYS: usize = 400_000;

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    if mode < Mode::Advanced {
        return;
    }
    let mut visited = 0;
    for root in software_roots() {
        let mut pending = vec![root];
        while let Some(key) = pending.pop() {
            visited += 1;
            if visited > MAX_KEYS || (visited % 2_000 == 0 && context.progress.cancelled()) {
                found.warn(
                    "The deep registry search stopped early; some leftovers may be missed.".into(),
                );
                return;
            }
            inspect_values(identity, context, found, &key);
            if key.depth() >= MAX_DEPTH {
                continue;
            }
            for name in subkeys_or_empty(context.system, &key) {
                let child = key.child(&name);
                let folded = child.path.to_lowercase();
                if SKIP.contains(&folded.as_str()) {
                    continue;
                }
                if identity.matches(&name, Match::Contains) && !context.belongs_to_other(&name) {
                    found.key(
                        context,
                        child.clone(),
                        "Key whose name contains the program's name",
                        Confidence::Low,
                    );
                }
                pending.push(child);
            }
        }
    }
    data_folders(identity, context, found);
}

fn inspect_values(identity: &Identity, context: &Context<'_>, found: &mut Found, key: &RegPath) {
    let Ok(values) = context.system.values(key) else {
        return;
    };
    for value in values {
        let named = embedded_path(&value.name).is_some_and(|path| identity.owns_path(&path));
        let points = value
            .as_text()
            .is_some_and(|text| identity.references(&text));
        if named || points {
            found.value(
                context,
                key.clone(),
                value.name,
                "Value pointing into its folders",
                Confidence::Medium,
            );
        }
    }
}

/// Folders two levels down in the data roots whose names contain the program's.
fn data_folders(identity: &Identity, context: &Context<'_>, found: &mut Found) {
    let folders = context.folders;
    for root in [
        &folders.program_data,
        &folders.app_data,
        &folders.local_app_data,
        &folders.local_low,
    ] {
        for parent in list(root).dirs {
            for dir in list(&parent).dirs {
                let name = file_name(&dir);
                if identity.matches(&name, Match::Contains) && !context.belongs_to_other(&name) {
                    found.path(
                        context,
                        identity,
                        dir,
                        "Data folder whose name contains the program's name",
                        Confidence::Low,
                    );
                }
            }
        }
    }
}
