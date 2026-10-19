//! COM classes, type libraries and file types whose server or open command
//! lives in the program's folders (Moderate and up).

use super::{Confidence, Context, Found, Mode};
use crate::identity::Identity;
use crate::model::{Hive, RegPath};
use crate::system::{subkeys_or_empty, text_value, values_or_empty};

fn class_roots() -> [RegPath; 3] {
    [
        RegPath::new(Hive::LocalMachine, r"SOFTWARE\Classes"),
        RegPath::new(Hive::LocalMachine, r"SOFTWARE\Classes\WOW6432Node"),
        RegPath::new(Hive::CurrentUser, r"Software\Classes"),
    ]
}

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    if mode < Mode::Moderate || identity.install_dirs.is_empty() {
        return;
    }
    let mut classes = Vec::new();
    for root in class_roots() {
        if context.progress.cancelled() {
            return;
        }
        classes.extend(com_classes(identity, context, found, &root.child("CLSID")));
        type_libraries(identity, context, found, &root.child("TypeLib"));
        if root.path.ends_with("WOW6432Node") {
            continue;
        }
        file_types(identity, context, found, &root);
        // `Applications\<exe>` is shared by every program with an exe of that
        // name, so it counts only when it opens or shows one of ours.
        for exe in &identity.exe_names {
            let key = root.child("Applications").child(exe);
            let opens =
                identity.references(&default_text(context, &key.child(r"shell\open\command")));
            let shows = identity.references(&default_text(context, &key.child("DefaultIcon")));
            if opens || shows {
                found.key(
                    context,
                    key,
                    "Registered application entry",
                    Confidence::High,
                );
            }
        }
        if let Some(exe) = &identity.main_exe {
            let key = root.child("AppID").child(exe);
            if crate::system::key_exists(context.system, &key) {
                found.key(
                    context,
                    key,
                    "COM security entry for its executable",
                    Confidence::Medium,
                );
            }
        }
    }
    shell_extensions(context, found, &classes);
    if mode == Mode::Advanced {
        for root in class_roots() {
            interfaces(context, found, &root, &classes);
            app_ids(context, found, &root, &classes);
        }
    }
}

/// Interfaces whose proxy is one of the program's COM classes.
fn interfaces(context: &Context<'_>, found: &mut Found, root: &RegPath, classes: &[String]) {
    let interface = root.child("Interface");
    for name in subkeys_or_empty(context.system, &interface) {
        let key = interface.child(&name);
        let proxy = default_text(context, &key.child("ProxyStubClsid32"));
        if classes
            .iter()
            .any(|id| id.eq_ignore_ascii_case(proxy.trim()))
        {
            found.key(
                context,
                key,
                "COM interface served by one of its classes",
                Confidence::High,
            );
        }
    }
}

/// The application identity a program's COM classes share.
fn app_ids(context: &Context<'_>, found: &mut Found, root: &RegPath, classes: &[String]) {
    for id in classes {
        let class = root.child("CLSID").child(id);
        let Some(app_id) = text_value(&values_or_empty(context.system, &class), "AppID") else {
            continue;
        };
        let key = root.child("AppID").child(app_id.trim());
        if crate::system::key_exists(context.system, &key) {
            found.key(
                context,
                key,
                "COM application identity of its classes",
                Confidence::Medium,
            );
        }
    }
}

fn default_text(context: &Context<'_>, key: &RegPath) -> String {
    let text = text_value(&values_or_empty(context.system, key), "").unwrap_or_default();
    crate::paths::expand(context.system, &text)
}

/// COM classes whose server is one of the program's files; returns their ids.
pub(super) fn com_classes(
    identity: &Identity,
    context: &Context<'_>,
    found: &mut Found,
    clsid: &RegPath,
) -> Vec<String> {
    let mut ours = Vec::new();
    for name in subkeys_or_empty(context.system, clsid) {
        let class = clsid.child(&name);
        let server = ["InprocServer32", "LocalServer32"]
            .into_iter()
            .any(|kind| identity.references(&default_text(context, &class.child(kind))));
        if server {
            found.key(
                context,
                class,
                "COM class served from its files",
                Confidence::High,
            );
            ours.push(name);
        }
    }
    ours
}

/// Where Explorer loads shell extensions (context menu, drag and drop,
/// property pages) by class id.
const HANDLER_CLASSES: &[&str] = &[
    "*",
    "AllFilesystemObjects",
    "Directory",
    r"Directory\Background",
    "Drive",
    "Folder",
];
const HANDLER_KINDS: &[&str] = &[
    "ContextMenuHandlers",
    "CopyHookHandlers",
    "DragDropHandlers",
    "PropertySheetHandlers",
];

/// Explorer's registrations of the program's COM classes as shell
/// extensions, which would otherwise point at a class that no longer exists.
fn shell_extensions(context: &Context<'_>, found: &mut Found, classes: &[String]) {
    if classes.is_empty() {
        return;
    }
    let ours = |text: &str| {
        classes
            .iter()
            .any(|id| id.eq_ignore_ascii_case(text.trim()))
    };
    for root in class_roots() {
        for class in HANDLER_CLASSES {
            for kind in HANDLER_KINDS {
                let handlers = root.child(class).child("shellex").child(kind);
                for name in subkeys_or_empty(context.system, &handlers) {
                    let key = handlers.child(&name);
                    if ours(&name) || ours(&default_text(context, &key)) {
                        found.key(
                            context,
                            key,
                            "Explorer extension registration",
                            Confidence::High,
                        );
                    }
                }
            }
        }
    }
    for base in ["SOFTWARE", r"SOFTWARE\WOW6432Node"] {
        let approved = RegPath::new(
            Hive::LocalMachine,
            &format!(r"{base}\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved"),
        );
        for value in values_or_empty(context.system, &approved) {
            if ours(&value.name) {
                let reason = "Explorer's approval of its extension";
                found.value(
                    context,
                    approved.clone(),
                    value.name,
                    reason,
                    Confidence::High,
                );
            }
        }
    }
}

/// Whose file a type library's registration names.
#[derive(PartialEq)]
enum Owner {
    Ours,
    Theirs,
    /// Nothing registered, or a file that is already gone.
    Nobody,
}

fn type_libraries(
    identity: &Identity,
    context: &Context<'_>,
    found: &mut Found,
    typelib: &RegPath,
) {
    let reason = "Type library stored in its files";
    for name in subkeys_or_empty(context.system, typelib) {
        let library = typelib.child(&name);
        // Each version with its locales and who owns their files.
        let mut versions = Vec::new();
        for version in subkeys_or_empty(context.system, &library) {
            let version = library.child(&version);
            let mut locales = Vec::new();
            for locale in subkeys_or_empty(context.system, &version) {
                let locale = version.child(&locale);
                locales.push((owner(identity, context, &locale), locale));
            }
            versions.push((version, locales));
        }
        let owners = || versions.iter().flat_map(|(_, locales)| locales.iter());
        if !owners().any(|(owner, _)| *owner == Owner::Ours) {
            continue;
        }
        // The whole GUID only when every version in it is ours (or empty);
        // otherwise just the versions, or locales, that are, so another
        // program's registration of the same library survives.
        if owners().all(|(owner, _)| *owner != Owner::Theirs) {
            found.key(context, library, reason, Confidence::High);
            continue;
        }
        for (version, locales) in versions {
            if locales.iter().all(|(owner, _)| *owner != Owner::Theirs) {
                if locales.iter().any(|(owner, _)| *owner == Owner::Ours) {
                    found.key(context, version, reason, Confidence::High);
                }
                continue;
            }
            for (owner, locale) in locales {
                if owner == Owner::Ours {
                    found.key(context, locale, reason, Confidence::High);
                }
            }
        }
    }
}

fn owner(identity: &Identity, context: &Context<'_>, locale: &RegPath) -> Owner {
    let mut owner = Owner::Nobody;
    for arch in ["win32", "win64"] {
        let file = default_text(context, &locale.child(arch));
        if identity.references(&file) {
            owner = Owner::Ours;
        } else if crate::paths::embedded_path(&file).is_some_and(|path| path.exists()) {
            return Owner::Theirs;
        }
    }
    owner
}

/// Document types (`AcmeWidget.Document`) whose open command or icon is one
/// of the program's files. Extension keys themselves are shared and kept.
pub(super) fn file_types(
    identity: &Identity,
    context: &Context<'_>,
    found: &mut Found,
    classes: &RegPath,
) {
    for name in subkeys_or_empty(context.system, classes) {
        if name.starts_with('.') || name.contains('{') {
            continue;
        }
        let class = classes.child(&name);
        if identity.references(&default_text(context, &class.child(r"shell\open\command"))) {
            found.key(
                context,
                class,
                "File type opened by the program",
                Confidence::High,
            );
        } else if identity.references(&default_text(context, &class.child("DefaultIcon"))) {
            found.key(
                context,
                class,
                "File type showing the program's icon",
                Confidence::Medium,
            );
        }
    }
}
