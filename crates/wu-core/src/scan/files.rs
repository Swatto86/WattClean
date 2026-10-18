//! Leftover folders and shortcuts on disk.

use std::path::{Path, PathBuf};

use super::shortcut::{target_of, url_of};
use super::{Confidence, Context, Found, Mode, Target};
use crate::identity::{Identity, Match};
use crate::paths::is_within;

pub(super) fn scan(identity: &Identity, mode: Mode, context: &Context<'_>, found: &mut Found) {
    for dir in &identity.install_dirs {
        if dir.is_dir() {
            found.path(
                context,
                identity,
                dir.clone(),
                "Install folder left behind",
                Confidence::High,
            );
        }
    }
    let folders = context.folders;
    let mut roots: Vec<PathBuf> = folders
        .program_roots()
        .into_iter()
        .map(Path::to_path_buf)
        .collect();
    roots.push(folders.local_app_data.join("Programs"));
    roots.extend(folders.common_files.iter().cloned());
    for root in &roots {
        if context.progress.cancelled() {
            return;
        }
        named_children(identity, mode, context, found, root);
    }
    for menu in folders.start_menus() {
        start_menu(identity, mode, context, found, menu);
    }
    let pinned = folders.quick_launch().join(r"User Pinned\TaskBar");
    for place in [
        folders.desktop.clone(),
        folders.public_desktop.clone(),
        folders.quick_launch(),
        pinned,
    ] {
        for file in list(&place).files {
            shortcut(identity, mode, context, found, &file);
        }
    }
    if mode == Mode::Advanced {
        for entry in list(&folders.temp).all() {
            if identity.matches(&stem(&entry), Match::Contains)
                && !context.belongs_to_other(&stem(&entry))
            {
                found.path(
                    context,
                    identity,
                    entry,
                    "Temporary file named after the program",
                    Confidence::Low,
                );
            }
        }
    }
}

/// Folders in `root` named after the program, or after its publisher with
/// the program's folder inside.
pub(super) fn named_children(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    root: &Path,
) {
    for dir in list(root).dirs {
        let name = file_name(&dir);
        if identity.matches(&name, mode.name_match()) && !context.belongs_to_other(&name) {
            found.path(
                context,
                identity,
                dir,
                "Folder named after the program",
                strength(identity, &name),
            );
        } else if identity.is_publisher(&name) {
            publisher_folder(identity, mode, context, found, &dir);
        } else if let Some(level) = exe_folder(identity, &name).filter(|_| mode >= Mode::Moderate) {
            found.path(
                context,
                identity,
                dir,
                "Folder named after one of the program's executables",
                level,
            );
        }
    }
}

fn publisher_folder(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    publisher: &Path,
) {
    let children = list(publisher);
    let mut matched = 0;
    for dir in &children.dirs {
        let name = file_name(dir);
        if identity.matches_inner(&name, mode.name_match()) && !context.belongs_to_other(&name) {
            found.path(
                context,
                identity,
                dir.clone(),
                "Program folder inside the publisher's folder",
                strength(identity, &name),
            );
            if found.contains(&Target::Folder { path: dir.clone() }) {
                matched += 1;
            }
        }
    }
    // The publisher's folder itself goes only when it holds nothing else
    // (just this program, or nothing because the uninstaller emptied it) and
    // no other installed program comes from the same publisher.
    if context.publisher_shared(identity) || !children.files.is_empty() {
        return;
    }
    if children.dirs.is_empty() {
        found.path(
            context,
            identity,
            publisher.to_path_buf(),
            "Empty publisher folder",
            Confidence::Medium,
        );
    } else if matched == children.dirs.len() {
        found.path(
            context,
            identity,
            publisher.to_path_buf(),
            "Publisher folder holding only this program",
            Confidence::Medium,
        );
    }
}

fn start_menu(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    menu: &Path,
) {
    let top = list(menu);
    for file in &top.files {
        shortcut(identity, mode, context, found, file);
    }
    for dir in top.dirs {
        let name = file_name(&dir);
        if identity.matches(&name, mode.name_match()) && !context.belongs_to_other(&name) {
            found.path(
                context,
                identity,
                dir,
                "Start menu folder",
                strength(identity, &name),
            );
        } else if identity.is_publisher(&name) {
            publisher_menu_folder(identity, mode, context, found, &dir);
        } else {
            menu_shortcuts(identity, mode, context, found, &dir, 2);
        }
    }
}

/// Shortcuts in a Start Menu folder and up to `depth` levels below it.
fn menu_shortcuts(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    dir: &Path,
    depth: usize,
) {
    let listing = list(dir);
    for file in &listing.files {
        shortcut(identity, mode, context, found, file);
    }
    if depth == 0 {
        return;
    }
    for sub in listing.dirs {
        menu_shortcuts(identity, mode, context, found, &sub, depth - 1);
    }
}

/// The publisher's Start Menu folder: its program subfolders as usual, every
/// shortcut in it, and the folder itself when all it holds is the program's.
fn publisher_menu_folder(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    dir: &Path,
) {
    publisher_folder(identity, mode, context, found, dir);
    menu_shortcuts(identity, mode, context, found, dir, 2);
    if context.publisher_shared(identity) || !only_ours(found, dir, 2) {
        return;
    }
    found.path(
        context,
        identity,
        dir.to_path_buf(),
        "Start menu folder holding only the program's shortcuts",
        Confidence::High,
    );
}

/// Everything in `dir` (to `depth` levels down) was found to be the
/// program's, and there is something in it.
fn only_ours(found: &Found, dir: &Path, depth: usize) -> bool {
    let listing = list(dir);
    let files_ours = listing
        .files
        .iter()
        .all(|file| found.contains(&Target::File { path: file.clone() }));
    let dirs_ours = listing.dirs.iter().all(|sub| {
        found.contains(&Target::Folder { path: sub.clone() })
            || (depth > 0 && only_ours(found, sub, depth - 1))
    });
    (!listing.files.is_empty() || !listing.dirs.is_empty()) && files_ours && dirs_ours
}
/// A shortcut whose target lies in the install folder, or (by name) one
/// called after the program.
fn shortcut(
    identity: &Identity,
    mode: Mode,
    context: &Context<'_>,
    found: &mut Found,
    file: &Path,
) {
    let extension = file
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if extension != "lnk" && extension != "url" {
        return;
    }
    if let Some(target) = target_of(file) {
        if identity
            .install_dirs
            .iter()
            .any(|dir| is_within(&target, dir))
        {
            found.path(
                context,
                identity,
                file.to_path_buf(),
                "Shortcut into the install folder",
                Confidence::High,
            );
        }
        return;
    }
    if let Some(host) = url_of(file).as_deref().and_then(crate::identity::host_of)
        && identity.website_host.as_deref() == Some(host.as_str())
    {
        found.path(
            context,
            identity,
            file.to_path_buf(),
            "Web shortcut to the program's site",
            Confidence::High,
        );
        return;
    }
    let name = stem(file);
    if identity.matches(&name, mode.name_match()) && !context.belongs_to_other(&name) {
        found.path(
            context,
            identity,
            file.to_path_buf(),
            "Shortcut named after the program",
            strength(identity, &name),
        );
    }
}

/// A folder named exactly after one of the program's executables (`vlc`
/// for vlc.exe): likely its data when that is the program's own executable,
/// only possible for a helper it bundles (another tool may own the name).
fn exe_folder(identity: &Identity, name: &str) -> Option<Confidence> {
    let key = crate::identity::name_key(name);
    let stem = |exe: &str| crate::identity::name_key(exe.trim_end_matches(".exe"));
    if crate::identity::is_generic(&key) {
        return None;
    }
    if identity
        .main_exe
        .as_deref()
        .is_some_and(|exe| stem(exe) == key)
    {
        Some(Confidence::Medium)
    } else if identity.exe_names.iter().any(|exe| stem(exe) == key) {
        Some(Confidence::Low)
    } else {
        None
    }
}

/// An exact name match is strong evidence; a looser one is weaker.
fn strength(identity: &Identity, name: &str) -> Confidence {
    if identity.matches_inner(name, Match::Exact) {
        Confidence::High
    } else if identity.matches_inner(name, Match::Prefix) {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

pub(super) struct Listing {
    pub dirs: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
}

impl Listing {
    fn all(self) -> Vec<PathBuf> {
        self.dirs.into_iter().chain(self.files).collect()
    }
}

/// Entries of `dir`, split by kind; links and junctions are skipped so a
/// scan never wanders out of the folder it was asked about.
pub(super) fn list(dir: &Path) -> Listing {
    let mut listing = Listing {
        dirs: Vec::new(),
        files: Vec::new(),
    };
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => listing.dirs.push(entry.path()),
            Ok(kind) if kind.is_file() => listing.files.push(entry.path()),
            _ => {}
        }
    }
    listing.dirs.sort();
    listing.files.sort();
    listing
}

pub(super) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
