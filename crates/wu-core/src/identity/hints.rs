//! Where else a program says where it lives: Windows Installer's component
//! records and the shortcuts Windows made for it. Read before the uninstaller
//! runs, while they still point at something.

use std::path::Path;

use super::Identity;
use crate::folders::Folders;
use crate::identity::Match;
use crate::msi;
use crate::scan::shortcut::target_of;
use crate::system::System;

impl Identity {
    /// Add the folders Windows Installer registered the product's files in,
    /// and the product name it recorded.
    pub fn add_msi_hints(&mut self, system: &dyn System, folders: &Folders) {
        let Some(packed) = self.msi_product_code.as_deref().and_then(msi::packed_guid) else {
            return;
        };
        if let Some(name) = msi::product_name(system, &packed) {
            self.add_name(&crate::identity::product_name(&name));
        }
        for dir in msi::product_dirs(system, &packed) {
            self.add_dir(dir, folders);
        }
    }

    /// Add the folders the program's Start Menu and desktop shortcuts point
    /// into, for an entry that names no install folder.
    pub fn add_shortcut_hints(&mut self, folders: &Folders) {
        if !self.install_dirs.is_empty() {
            return;
        }
        let mut places: Vec<&Path> = folders.start_menus().to_vec();
        places.extend(folders.desktops());
        for place in places {
            for file in shortcuts_below(place, 3) {
                let stem = file
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !self.matches_inner(&stem, Match::Exact) {
                    continue;
                }
                if let Some(dir) =
                    target_of(&file).and_then(|target| target.parent().map(Path::to_path_buf))
                {
                    self.add_dir(dir, folders);
                }
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
