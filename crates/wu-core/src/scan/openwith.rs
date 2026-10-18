//! Moderate and up: where Windows lists a program as a way to open a file
//! type (the type's open-with lists and default handler), for the document
//! types and executables found to be the program's. The `.ext` keys
//! themselves are shared and stay; only the entries naming the program go.

use super::{Confidence, Context, Found, Mode};
use crate::identity::Identity;
use crate::model::{Hive, RegPath};
use crate::system::{subkeys_or_empty, text_value, values_or_empty};

/// The program's document types (`AcmeWidget.Document`) found so far.
fn own_progids(found: &Found) -> Vec<String> {
    let mut progids: Vec<String> = found
        .items
        .iter()
        .filter_map(|(target, _, _)| match target {
            super::Target::Key { key } => Some(key),
            _ => None,
        })
        .filter(|key| {
            let parent = key.parent().map(|parent| parent.path.to_lowercase());
            parent.as_deref() == Some("software\\classes") && !key.name().starts_with('.')
        })
        .map(|key| key.name().to_lowercase())
        .collect();
    progids.sort();
    progids.dedup();
    progids
}

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    if mode < Mode::Moderate {
        return;
    }
    let progids = own_progids(found);
    if progids.is_empty() && identity.exe_names.is_empty() {
        return;
    }
    for classes in [
        RegPath::new(Hive::LocalMachine, r"SOFTWARE\Classes"),
        RegPath::new(Hive::CurrentUser, r"Software\Classes"),
    ] {
        for name in subkeys_or_empty(context.system, &classes) {
            if !name.starts_with('.') {
                continue;
            }
            let ext = classes.child(&name);
            extension(&progids, context, found, &ext);
        }
    }
    let file_exts = RegPath::new(
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts",
    );
    for name in subkeys_or_empty(context.system, &file_exts) {
        let ext = file_exts.child(&name);
        progid_values(&progids, context, found, &ext.child("OpenWithProgids"));
        let list = ext.child("OpenWithList");
        for value in values_or_empty(context.system, &list) {
            let exe = value.as_text().unwrap_or_default().to_lowercase();
            // A generic name (Update.exe, Launcher.exe) could be anyone's.
            let stem = crate::identity::name_key(exe.trim_end_matches(".exe"));
            if identity.exe_names.contains(&exe) && !crate::identity::is_generic(&stem) {
                found.value(
                    context,
                    list.clone(),
                    value.name,
                    "Open-with choice naming one of its executables",
                    Confidence::High,
                );
            }
        }
    }
}

/// One `.ext` class: its open-with list, and its default handler when that
/// is the program's own document type (leaving the type with no handler).
fn extension(progids: &[String], context: &Context<'_>, found: &mut Found, ext: &RegPath) {
    progid_values(progids, context, found, &ext.child("OpenWithProgids"));
    let default = text_value(&values_or_empty(context.system, ext), "").unwrap_or_default();
    if progids.contains(&default.to_lowercase()) {
        found.value(
            context,
            ext.clone(),
            String::new(),
            "Default handler of the file type is its document type",
            Confidence::Medium,
        );
    }
}

fn progid_values(progids: &[String], context: &Context<'_>, found: &mut Found, key: &RegPath) {
    for value in values_or_empty(context.system, key) {
        if progids.contains(&value.name.to_lowercase()) {
            found.value(
                context,
                key.clone(),
                value.name,
                "Open-with entry naming its document type",
                Confidence::High,
            );
        }
    }
}
