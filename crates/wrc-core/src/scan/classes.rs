//! The `Classes` keys: COM registrations, type libraries, file types,
//! right-click commands and file extensions.

use super::programs::all_commands_missing;
use super::{Category, Context, Found};
use crate::check::{PathCheck, Verdict};
use crate::model::RegPath;
use crate::paths::command_target;
use crate::system::{key_exists, subkeys_or_empty, text_value, values_or_empty};

/// `Classes` keys that are Windows' own structure, never a file type.
const STRUCTURE: [&str; 18] = [
    "*",
    "ActivatableClasses",
    "AllFilesystemObjects",
    "AppID",
    "Applications",
    "CLSID",
    "DesktopBackground",
    "Directory",
    "Drive",
    "Folder",
    "Installer",
    "Interface",
    "Local Settings",
    "MIME",
    "PackagedCom",
    "SystemFileAssociations",
    "TypeLib",
    "WOW6432Node",
];

/// Where File Explorer's right-click commands live in each `Classes` key.
const SHELL_PARENTS: [&str; 8] = [
    "*",
    "AllFilesystemObjects",
    "Directory",
    r"Directory\Background",
    "Drive",
    "Folder",
    "DesktopBackground",
    r"LibraryFolder\Background",
];

/// COM classes whose servers are all missing, and type libraries whose
/// files are all missing.
pub fn com_objects(context: &Context, out: &mut Vec<Found>) {
    for classes in context.classes_roots() {
        for (clsid, view32) in [
            (classes.child("CLSID"), false),
            (classes.child(r"WOW6432Node\CLSID"), true),
        ] {
            for id in subkeys_or_empty(context.system, &clsid) {
                if context.progress.cancelled() {
                    return;
                }
                let key = clsid.child(&id);
                let servers: Vec<String> = ["InprocServer32", "LocalServer32"]
                    .iter()
                    .filter_map(|server| {
                        let values = values_or_empty(context.system, &key.child(server));
                        text_value(&values, "").and_then(|text| command_target(&text))
                    })
                    .collect();
                if let Some(first) = all_missing(context.check_for(&key), &servers, view32) {
                    let name = text_value(&values_or_empty(context.system, &key), "")
                        .unwrap_or_else(|| id.clone());
                    out.push(Found::key(
                        Category::ComObjects,
                        key,
                        format!(
                            "COM class \"{name}\" is served by {first}, which no longer exists."
                        ),
                    ));
                }
            }
        }
        type_libraries(context, &classes.child("TypeLib"), out);
    }
}

fn type_libraries(context: &Context, typelib: &RegPath, out: &mut Vec<Found>) {
    for id in subkeys_or_empty(context.system, typelib) {
        if context.progress.cancelled() {
            return;
        }
        let key = typelib.child(&id);
        let mut files = Vec::new();
        for version in subkeys_or_empty(context.system, &key) {
            let version_key = key.child(&version);
            for locale in subkeys_or_empty(context.system, &version_key) {
                let locale_key = version_key.child(&locale);
                for platform in subkeys_or_empty(context.system, &locale_key) {
                    let values = values_or_empty(context.system, &locale_key.child(&platform));
                    // A file's path, never a command line: no arguments to cut.
                    let path = text_value(&values, "")
                        .map(|text| text.trim().trim_matches('"').to_owned())
                        .filter(|text| !text.is_empty());
                    if let Some(path) = path {
                        files.push((path, platform.eq_ignore_ascii_case("win32")));
                    }
                }
            }
        }
        let judged = !files.is_empty()
            && files
                .iter()
                .all(|(path, view32)| library_missing(context.check_for(&key), path, *view32));
        if judged {
            out.push(Found::key(
                Category::ComObjects,
                key,
                format!("Type library file {} no longer exists.", files[0].0),
            ));
        }
    }
}

/// Type library data may name a resource inside a file (`updater.exe\4`),
/// so the library is missing only when the file holding it is missing too.
fn library_missing(check: &PathCheck, path: &str, view32: bool) -> bool {
    if check.path(path, view32) != Verdict::Missing {
        return false;
    }
    match path.rsplit_once('\\') {
        Some((file, index)) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => {
            check.path(file, view32) == Verdict::Missing
        }
        _ => true,
    }
}

/// The first of `paths` when there is at least one and every one is missing.
fn all_missing(check: &PathCheck, paths: &[String], view32: bool) -> Option<String> {
    let first = paths.first()?;
    paths
        .iter()
        .all(|path| check.path(path, view32) == Verdict::Missing)
        .then(|| first.clone())
}

/// File types (ProgIDs) whose every command names a missing program.
pub fn class_keys(context: &Context, out: &mut Vec<Found>) {
    for classes in context.classes_roots() {
        for name in subkeys_or_empty(context.system, &classes) {
            if context.progress.cancelled() {
                return;
            }
            if name.starts_with('.')
                || STRUCTURE
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            let key = classes.child(&name);
            let Some(target) = all_commands_missing(context, &key) else {
                continue;
            };
            let icon = values_or_empty(context.system, &key.child("DefaultIcon"));
            let icon_present = text_value(&icon, "")
                .and_then(|text| command_target(&text))
                .is_some_and(|path| context.check_for(&key).path(&path, false) != Verdict::Missing);
            if !icon_present {
                out.push(Found::key(
                    Category::ClassKeys,
                    key,
                    format!("File type \"{name}\" opens with {target}, which no longer exists."),
                ));
            }
        }
    }
}

/// Right-click commands whose program is missing.
pub fn context_menus(context: &Context, out: &mut Vec<Found>) {
    for classes in context.classes_roots() {
        let mut shells: Vec<RegPath> = SHELL_PARENTS
            .iter()
            .map(|parent| classes.child(parent).child("shell"))
            .collect();
        let associations = classes.child("SystemFileAssociations");
        shells.extend(
            subkeys_or_empty(context.system, &associations)
                .iter()
                .map(|kind| associations.child(kind).child("shell")),
        );
        for shell in shells {
            for verb in subkeys_or_empty(context.system, &shell) {
                let key = shell.child(&verb);
                let values = values_or_empty(context.system, &key.child("command"));
                if values
                    .iter()
                    .any(|value| value.name.eq_ignore_ascii_case("DelegateExecute"))
                {
                    continue;
                }
                let Some(target) = text_value(&values, "").and_then(|text| command_target(&text))
                else {
                    continue;
                };
                if context.check_for(&key).missing(&target, false) {
                    let label = text_value(&values_or_empty(context.system, &key), "")
                        .unwrap_or_else(|| verb.clone());
                    out.push(Found::key(
                        Category::ContextMenus,
                        key,
                        format!("\"{label}\" runs {target}, which no longer exists."),
                    ));
                }
            }
        }
    }
}

/// Extension keys that hold nothing but a file type no longer registered,
/// or nothing at all.
pub fn extensions(context: &Context, out: &mut Vec<Found>) {
    // Windows merges the machine's classes with each account's, so a file
    // type defined in any of them may be the one an extension names.
    let roots = context.classes_roots();
    for classes in roots.clone() {
        for name in subkeys_or_empty(context.system, &classes) {
            if !name.starts_with('.') || name.len() < 2 {
                continue;
            }
            let key = classes.child(&name);
            if !subkeys_or_empty(context.system, &key).is_empty() {
                continue;
            }
            let values = values_or_empty(context.system, &key);
            if values.iter().any(|value| !value.name.is_empty()) {
                continue;
            }
            let detail = match text_value(&values, "") {
                None => format!("Extension {name} holds nothing."),
                Some(kind) => {
                    let registered = roots
                        .iter()
                        .any(|root| key_exists(context.system, &root.child(&kind)));
                    if registered {
                        continue;
                    }
                    format!("Extension {name} names file type \"{kind}\", which is not registered.")
                }
            };
            out.push(Found::key(Category::Extensions, key, detail));
        }
    }
}
