//! Where else a program says where it lives: Windows Installer's component
//! records and the shortcuts Windows made for it. Read before the uninstaller
//! runs, while they still point at something.

use std::path::{Path, PathBuf};

use super::Identity;
use crate::folders::Folders;
use crate::identity::Match;
use crate::msi::{self, ComponentIndex};
use crate::scan::shortcut::target_of;
use crate::system::System;

/// Both hints for every installed program, read once: Windows Installer's
/// components, and each shortcut's name with the folder it points into.
pub struct Hints {
    components: ComponentIndex,
    shortcuts: Vec<(String, PathBuf)>,
}

impl Hints {
    pub fn load(system: &dyn System, folders: &Folders) -> Self {
        Self {
            components: ComponentIndex::load(system),
            shortcuts: shortcut_dirs(folders),
        }
    }
}

/// Each Start Menu and desktop shortcut's name, and the folder it points into.
fn shortcut_dirs(folders: &Folders) -> Vec<(String, PathBuf)> {
    let mut places: Vec<&Path> = folders.start_menus().to_vec();
    places.extend(folders.desktops());
    places
        .into_iter()
        .flat_map(|place| shortcuts_below(place, 3))
        .filter_map(|file| {
            let stem = file.file_stem()?.to_string_lossy().into_owned();
            let dir = target_of(&file)?.parent()?.to_path_buf();
            Some((stem, dir))
        })
        .collect()
}

impl Identity {
    /// Add the folders Windows Installer registered the product's files in,
    /// and the product name it recorded.
    pub fn add_msi_hints(&mut self, system: &dyn System, folders: &Folders) {
        self.add_components(system, &ComponentIndex::load(system), folders);
    }

    /// Add the folders the program's Start Menu and desktop shortcuts point
    /// into, for an entry that names no install folder.
    pub fn add_shortcut_hints(&mut self, folders: &Folders) {
        self.add_shortcuts(&shortcut_dirs(folders), folders);
    }

    /// Both hints, from ones read for every program at once.
    pub fn add_hints(&mut self, system: &dyn System, hints: &Hints, folders: &Folders) {
        self.add_components(system, &hints.components, folders);
        self.add_shortcuts(&hints.shortcuts, folders);
    }

    fn add_components(&mut self, system: &dyn System, index: &ComponentIndex, folders: &Folders) {
        let Some(packed) = self.msi_product_code.as_deref().and_then(msi::packed_guid) else {
            return;
        };
        if let Some(name) = msi::product_name(system, &packed) {
            self.add_name(&crate::identity::product_name(&name));
        }
        for dir in index.product_dirs(system, &packed) {
            self.add_dir(dir, folders);
        }
    }

    fn add_shortcuts(&mut self, shortcuts: &[(String, PathBuf)], folders: &Folders) {
        if !self.install_dirs.is_empty() {
            return;
        }
        for (stem, dir) in shortcuts {
            if self.matches_inner(stem, Match::Exact) {
                self.add_dir(dir.clone(), folders);
            }
        }
    }
}

/// `.lnk` files in `dir` and up to `depth` levels of folders below it.
pub(crate) fn shortcuts_below(dir: &Path, depth: usize) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![(dir.to_path_buf(), 0usize)];
    while let Some((folder, level)) = pending.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() && level < depth => pending.push((path, level + 1)),
                Ok(kind) if kind.is_file() => {
                    let lnk = path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
                    if lnk {
                        found.push(path);
                    }
                }
                _ => {}
            }
        }
    }
    found.sort();
    found
}
