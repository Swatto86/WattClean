//! Windows Installer's records of an MSI product: the product and feature
//! keys (every mode), and in Moderate the component and upgrade-code
//! entries that make Windows Installer believe files it has lost are still
//! installed.

use super::{Confidence, Context, Found, Mode};
use crate::identity::Identity;
use crate::model::{Hive, RegPath};
use crate::msi::{packed_guid, upgrade_entries};
use crate::system::key_exists;

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    if let Some(bundle) = &identity.bundle {
        bundle_leftovers(identity, context, found, bundle);
    }
    let Some(packed) = identity.msi_product_code.as_deref().and_then(packed_guid) else {
        return;
    };
    // Still registered as an uninstall entry means Windows Installer believes
    // the product is installed; removing its records is then a forced removal.
    let still_installed = identity
        .uninstall_key
        .as_ref()
        .is_some_and(|key| key_exists(context.system, key));
    let level = if still_installed {
        Confidence::Medium
    } else {
        Confidence::High
    };
    let hklm = |path: &str| RegPath::new(Hive::LocalMachine, path);
    let hkcu = |path: &str| RegPath::new(Hive::CurrentUser, path);
    for key in [
        hklm(&format!(r"SOFTWARE\Classes\Installer\Products\{packed}")),
        hklm(&format!(r"SOFTWARE\Classes\Installer\Features\{packed}")),
        hklm(&format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Products\{packed}"
        )),
        hkcu(&format!(r"Software\Microsoft\Installer\Products\{packed}")),
        hkcu(&format!(r"Software\Microsoft\Installer\Features\{packed}")),
    ] {
        if key_exists(context.system, &key) {
            found.key(
                context,
                key,
                "Windows Installer record of the product",
                level,
            );
        }
    }
    if mode < Mode::Moderate {
        return;
    }
    for (key, _) in crate::msi::component_files(context.system, &packed) {
        found.value(
            context,
            key,
            packed.clone(),
            "Windows Installer component record of one of its files",
            level,
        );
    }
    for (key, value) in upgrade_entries(context.system, &packed) {
        found.value(
            context,
            key,
            value.name,
            "Windows Installer upgrade code naming the product",
            level,
        );
    }
}

/// A bundle installer's cached setup under Package Cache, its dependency
/// provider record, its entries under the packages it depended on, and the
/// cache of a package no installed product or other bundle needs any more.
fn bundle_leftovers(
    identity: &Identity,
    context: &Context<'_>,
    found: &mut Found,
    bundle: &crate::identity::Bundle,
) {
    let folders = context.folders;
    let caches = [
        folders.program_data.join("Package Cache"),
        folders.local_app_data.join("Package Cache"),
    ];
    for cache in &caches {
        let dir = cache.join(&bundle.id);
        if dir.is_dir() {
            found.path(
                context,
                identity,
                dir,
                "Bundle installer's cached setup",
                Confidence::High,
            );
        }
    }
    if let Some(dir) = bundle.cache_dir.clone().filter(|dir| dir.is_dir()) {
        found.path(
            context,
            identity,
            dir,
            "Bundle installer's cached setup",
            Confidence::High,
        );
    }
    let dependencies = [
        RegPath::new(
            Hive::LocalMachine,
            r"SOFTWARE\Classes\Installer\Dependencies",
        ),
        RegPath::new(
            Hive::CurrentUser,
            r"Software\Classes\Installer\Dependencies",
        ),
    ];
    for root in &dependencies {
        if let Some(provider) = &bundle.provider {
            let key = root.child(provider);
            if key_exists(context.system, &key) {
                found.key(context, key, "Bundle dependency record", Confidence::High);
            }
        }
        for package in crate::system::subkeys_or_empty(context.system, root) {
            let dependents = root.child(&package).child("Dependents");
            let entry = dependents.child(&bundle.id);
            if !key_exists(context.system, &entry) {
                continue;
            }
            found.key(
                context,
                entry,
                "Bundle's dependency on a package",
                Confidence::High,
            );
            let others_depend = crate::system::subkeys_or_empty(context.system, &dependents)
                .iter()
                .any(|id| !id.eq_ignore_ascii_case(&bundle.id));
            let installed = context
                .others
                .iter()
                .any(|other| other.key.name().eq_ignore_ascii_case(&package));
            if others_depend || installed {
                continue;
            }
            // `{ProductCode}v1.2.3`: the package's cached installer.
            let prefix = format!("{}v", package.to_lowercase());
            for cache in &caches {
                for dir in super::files::list(cache).dirs {
                    let name = super::files::file_name(&dir).to_lowercase();
                    if name.starts_with(&prefix) {
                        found.path(
                            context,
                            identity,
                            dir,
                            "Cached package nothing else depends on",
                            Confidence::Medium,
                        );
                    }
                }
            }
        }
    }
}
