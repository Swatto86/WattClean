//! What a scan may never offer for deletion, whatever it matched.
//!
//! The rules are deliberately blunt: Windows' own trees, the roots every
//! program shares, user documents, and anything another installed program
//! still uses are out of bounds. A leftover that is missed costs a few
//! kilobytes; one wrongly deleted can break Windows or another program.

mod keys;

use std::path::{Path, PathBuf};

use super::{Confidence, Context, Found, Target};
use crate::identity::Identity;
use crate::model::RegPath;
use crate::paths::is_within;

pub use keys::{key_allowed, value_allowed};

/// Folders that are never deleted, nor any folder above them.
fn protected_folders(context: &Context<'_>) -> Vec<PathBuf> {
    let folders = context.folders;
    let mut list = vec![
        folders.windows.clone(),
        folders.program_files.clone(),
        folders.program_data.clone(),
        folders.users.clone(),
        folders.user_profile.clone(),
        folders.user_profile.join("AppData"),
        folders.app_data.clone(),
        folders.local_app_data.clone(),
        folders.local_low.clone(),
        folders.local_app_data.join("Programs"),
        folders.temp.clone(),
        folders.desktop.clone(),
        folders.public_desktop.clone(),
        folders.start_menu_programs.clone(),
        folders.common_start_menu_programs.clone(),
        folders.start_menu_programs.join("Startup"),
        folders.common_start_menu_programs.join("Startup"),
        folders.documents.clone(),
        folders.quick_launch(),
        folders.quick_launch().join(r"User Pinned\TaskBar"),
    ];
    list.extend(folders.program_files_x86.clone());
    list.extend(folders.common_files.iter().cloned());
    list.extend(folders.downloads.clone());
    list
}

pub fn path_allowed(context: &Context<'_>, identity: &Identity, path: &Path) -> bool {
    let folders = context.folders;
    let dotted = path.components().any(|part| {
        matches!(
            part,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    });
    if !path.is_absolute() || path.parent().is_none() || dotted {
        return false;
    }
    // The app's own folders, whatever a scan matched.
    if context
        .protected
        .iter()
        .any(|own| is_within(path, own) || is_within(own, path))
    {
        return false;
    }
    // A scheduled task's definition file, never Microsoft's own tasks.
    let tasks = folders.windows.join(r"System32\Tasks");
    if is_within(path, &tasks) {
        return path.is_file()
            && !is_within(path, &tasks.join("Microsoft"))
            && !others_use(context, path);
    }
    // Another account's data folders, never its profile or documents. The
    // Public and Default profiles are not accounts and follow the usual rules.
    if let Some(allowed) = other_user_data(folders, path) {
        return allowed && !others_use(context, path);
    }
    if protected_folders(context)
        .iter()
        .any(|protected| is_within(protected, path))
    {
        return false;
    }
    let never = [
        Some(&folders.windows),
        Some(&folders.documents),
        folders.downloads.as_ref(),
    ];
    if never
        .into_iter()
        .flatten()
        .any(|place| is_within(path, place))
    {
        return false;
    }
    let shortcut_area = folders
        .start_menus()
        .into_iter()
        .chain(folders.desktops())
        .any(|area| is_within(path, area))
        || is_within(path, &folders.quick_launch());
    if !shortcut_area && !in_program_area(context, identity, path) {
        return false;
    }
    !others_use(context, path)
}

fn others_use(context: &Context<'_>, path: &Path) -> bool {
    context.others.iter().any(|other| {
        other
            .dirs
            .iter()
            .any(|dir| is_within(path, dir) || is_within(dir, path))
    })
}

/// `Users\<account>\AppData\<Roaming|Local|LocalLow>\<folder>…` of another
/// account: deep enough to be a program's own folder, in a data root.
fn other_user_data(folders: &crate::folders::Folders, path: &Path) -> Option<bool> {
    if !is_within(path, &folders.users) || is_within(path, &folders.user_profile) {
        return None;
    }
    let rest = path.strip_prefix(&folders.users).ok()?;
    let parts: Vec<String> = rest
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let account = parts.first().map(String::as_str).unwrap_or_default();
    if ["public", "default", "default user", "all users"].contains(&account) {
        return None;
    }
    let data_root = matches!(
        parts.get(2).map(String::as_str),
        Some("roaming" | "local" | "locallow")
    );
    let own_folder = parts
        .get(3)
        .is_some_and(|name| !name.starts_with("microsoft") && !name.starts_with("windows"));
    Some(
        parts.len() >= 4
            && parts.get(1).map(String::as_str) == Some("appdata")
            && data_root
            && own_folder,
    )
}

fn in_program_area(context: &Context<'_>, identity: &Identity, path: &Path) -> bool {
    let folders = context.folders;
    // A bundle installer's cache folder, one level inside Package Cache,
    // belongs to the bundle being removed; anything else there stays.
    for cache in [
        folders.program_data.join("Package Cache"),
        folders.local_app_data.join("Package Cache"),
    ] {
        if is_within(path, &cache) {
            let one_below = path
                .parent()
                .is_some_and(|parent| crate::paths::same(parent, &cache));
            return identity.bundle.is_some() && one_below;
        }
    }
    let vendor = [
        folders.program_data.join("Microsoft"),
        folders.app_data.join("Microsoft"),
        folders.local_app_data.join("Microsoft"),
        folders.local_app_data.join("Packages"),
        folders.program_files.join("WindowsApps"),
    ];
    if vendor.iter().any(|place| is_within(path, place)) {
        return false;
    }
    let mut areas: Vec<&Path> = folders.program_roots();
    areas.push(&folders.temp);
    let Some(root) = areas.into_iter().find(|area| is_within(path, area)) else {
        return false;
    };
    // Microsoft's and Windows' own folders in a shared root are off limits,
    // unless this program was installed there itself (VS Code, Teams).
    let first = path
        .strip_prefix(root)
        .ok()
        .and_then(|rest| rest.components().next())
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let system_named = first.starts_with("microsoft") || first.starts_with("windows");
    !system_named || identity.owns_path(path)
}

impl Found {
    pub(super) fn key(
        &mut self,
        context: &Context<'_>,
        key: RegPath,
        reason: &str,
        confidence: Confidence,
    ) {
        if key_allowed(context, &key) {
            self.add(Target::Key { key }, reason, confidence);
        }
    }

    pub(super) fn value(
        &mut self,
        context: &Context<'_>,
        key: RegPath,
        name: String,
        reason: &str,
        confidence: Confidence,
    ) {
        if value_allowed(context, &key) {
            self.add(Target::Value { key, name }, reason, confidence);
        }
    }

    pub(super) fn path(
        &mut self,
        context: &Context<'_>,
        identity: &Identity,
        path: PathBuf,
        reason: &str,
        confidence: Confidence,
    ) {
        if !path_allowed(context, identity, &path) {
            return;
        }
        let target = if path.is_dir() {
            Target::Folder { path }
        } else if path.is_file() {
            Target::File { path }
        } else {
            return;
        };
        self.add(target, reason, confidence);
    }
}
