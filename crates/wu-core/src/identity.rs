//! What a program is known by, gathered before its uninstaller runs (which
//! may delete the folder that told us its executable names).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::folders::Folders;
use crate::model::{Program, RegPath};
use crate::paths::{embedded_path, expand, is_within, same};
use crate::system::System;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub display_name: String,
    /// Folded name keys (see [`name_key`]) the program's folders and keys may carry.
    pub names: Vec<String>,
    /// Shorter names ("Widget" for "Acme Widget") that only identify the
    /// program inside its publisher's folder or key, never on their own.
    pub inner_names: Vec<String>,
    pub publisher: Option<String>,
    /// Folded publisher key, legal suffixes removed.
    pub publisher_key: Option<String>,
    pub install_dirs: Vec<PathBuf>,
    /// Lower-case executable file names found in the install folder.
    pub exe_names: Vec<String>,
    /// Lower-case file name of the program's own executable (its icon's).
    pub main_exe: Option<String>,
    pub uninstall_key: Option<RegPath>,
    pub msi_product_code: Option<String>,
    /// Host of the program's website (`acme.example`), for `.url` shortcuts.
    pub website_host: Option<String>,
    /// The program's website address (host and path, see `site_key`): a
    /// `.url` shortcut must lie on it, a shared host alone says nothing.
    #[serde(default)]
    pub website_url: Option<String>,
    /// Folders of other installed programs that sit inside this program's
    /// folders (a plugin pack): what lies in them is theirs, not this one's.
    #[serde(default)]
    pub others_dirs: Vec<PathBuf>,
    /// For a bundle installer: its id, its Package Cache folder and its
    /// dependency provider key, read before its uninstaller runs.
    pub bundle: Option<Bundle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    pub id: String,
    pub cache_dir: Option<PathBuf>,
    pub provider: Option<String>,
}

mod hints;
mod names;
mod site;

pub use hints::Hints;
pub use names::{is_generic, name_key, product_name, publisher_key};
pub use site::{on_site, site_key};

impl Identity {
    /// Build the identity of an installed program, reading its install
    /// folder for executable names while it still exists.
    pub fn of_program(system: &dyn System, folders: &Folders, program: &Program) -> Self {
        let mut identity = Self::of_entry(system, folders, program);
        identity.add_msi_hints(system, folders);
        identity.add_shortcut_hints(folders);
        identity.read_executables();
        identity
    }

    /// The identity from the registry entry alone, without reading files:
    /// cheap enough to build for every other installed program.
    pub fn of_entry(system: &dyn System, folders: &Folders, program: &Program) -> Self {
        let mut identity = Self::named(&program.name, program.publisher.as_deref(), Vec::new());
        if let Some(location) = &program.install_location {
            identity.add_dir(PathBuf::from(expand(system, location)), folders);
        }
        // An entry without InstallLocation still says where it lives through
        // its icon or uninstaller, unless those sit in a shared system place.
        for hint in [&program.icon, &program.uninstall_command]
            .into_iter()
            .flatten()
        {
            if let Some(dir) = embedded_path(&expand(system, hint))
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
                && identity.install_dirs.is_empty()
            {
                identity.add_dir(dir, folders);
            }
        }
        identity.uninstall_key = Some(program.registry_key.clone());
        identity.msi_product_code = program.msi_product_code.clone();
        identity.main_exe = program
            .icon
            .as_deref()
            .and_then(|icon| embedded_path(&expand(system, icon)))
            .and_then(|exe| {
                exe.file_name()
                    .map(|name| name.to_string_lossy().to_lowercase())
            })
            .filter(|name| name.ends_with(".exe"));
        if program.msi_product_code.is_none() {
            identity.add_name(program.registry_key.name());
        }
        identity.website_host = program.website.as_deref().and_then(host_of);
        identity.website_url = program.website.as_deref().and_then(site::site_key);
        if program.bundle_cache.is_some() || program.bundle_provider.is_some() {
            identity.bundle = Some(Bundle {
                id: program.registry_key.name().to_owned(),
                cache_dir: program
                    .bundle_cache
                    .as_deref()
                    .and_then(|cache| embedded_path(&expand(system, cache)))
                    .and_then(|setup| setup.parent().map(Path::to_path_buf)),
                provider: program.bundle_provider.clone(),
            });
        }
        identity
    }

    /// Identity for a forced uninstall: a name and, optionally, folders the
    /// program is known to own (which do not name it; see [`Self::add_dir`]).
    pub fn named(display_name: &str, publisher: Option<&str>, install_dirs: Vec<PathBuf>) -> Self {
        let mut identity = Self {
            display_name: display_name.to_owned(),
            names: Vec::new(),
            inner_names: Vec::new(),
            publisher: publisher.map(str::to_owned),
            publisher_key: publisher.and_then(publisher_key),
            exe_names: Vec::new(),
            main_exe: None,
            install_dirs,
            uninstall_key: None,
            msi_product_code: None,
            website_host: None,
            website_url: None,
            others_dirs: Vec::new(),
            bundle: None,
        };
        let product = product_name(display_name);
        identity.add_name(&product);
        identity.add_name(display_name);
        if let Some(publisher) = publisher {
            // "Mozilla Firefox" by "Mozilla" is also "Firefox" inside Mozilla's folder.
            let first = publisher.split_whitespace().next().unwrap_or_default();
            if let Some(rest) = product.strip_prefix(first) {
                identity.add_inner_name(rest);
            }
        }
        identity
    }

    /// Add an install folder, unless it is a shared place, a root, or inside
    /// a folder already known; a folder holding known ones replaces them.
    /// Its name is taken as the program's only where the folder says so.
    pub fn add_dir(&mut self, dir: PathBuf, folders: &Folders) {
        if let Some(dir) = push_dir(&mut self.install_dirs, dir, folders) {
            self.name_from_dir(&dir, folders);
        }
    }

    /// True when `dir` could be a program's own install folder: not a shared
    /// place, a root, Windows' own tree or a folder above one of them.
    pub fn is_own_folder(dir: &Path, folders: &Folders) -> bool {
        push_dir(&mut Vec::new(), dir.to_path_buf(), folders).is_some()
            && !crate::scan::guard::is_shared_folder(folders, dir)
    }

    /// `Program Files\Notepad++` names the program anywhere;
    /// `Program Files\Acme\Widget` names it under Acme only. A folder deeper
    /// inside (`Go\src\encoding\json`, `Widget\1.2.3`) names nothing: an
    /// installer that registers every file has hundreds of such folders.
    fn name_from_dir(&mut self, dir: &Path, folders: &Folders) {
        let (Some(name), Some(parent)) = (dir.file_name(), dir.parent()) else {
            return;
        };
        let name = name.to_string_lossy().into_owned();
        let parent_name = parent
            .file_name()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.is_publisher(&parent_name) {
            self.add_inner_name(&name);
        } else if is_root(parent, folders) {
            self.add_name(&name);
        }
    }

    /// Record the executables in the install folders; do this before the
    /// uninstaller runs, which usually deletes them.
    pub fn read_executables(&mut self) {
        self.exe_names = self
            .install_dirs
            .iter()
            .flat_map(|dir| executables(dir))
            .collect();
        self.exe_names.sort();
        self.exe_names.dedup();
    }

    pub fn add_name(&mut self, name: &str) {
        let key = name_key(name);
        // A product may share its maker's name (Discord by Discord Inc.).
        if !is_generic(&key) && !self.names.contains(&key) {
            self.inner_names.retain(|inner| *inner != key);
            self.names.push(key);
        }
    }

    fn add_inner_name(&mut self, name: &str) {
        let key = name_key(name);
        if !is_generic(&key) && !self.names.contains(&key) && !self.inner_names.contains(&key) {
            self.inner_names.push(key);
        }
    }

    /// Every name the program may carry, inner ones included; what other
    /// programs are protected by.
    pub fn all_names(&self) -> Vec<String> {
        self.names
            .iter()
            .chain(&self.inner_names)
            .cloned()
            .collect()
    }

    /// Does a folder or key named `name` belong to this program?
    pub fn matches(&self, name: &str, strength: Match) -> bool {
        matches_any(&self.names, name, strength)
    }

    /// Like [`Self::matches`], for a folder or key inside the publisher's.
    pub fn matches_inner(&self, name: &str, strength: Match) -> bool {
        matches_any(&self.names, name, strength) || matches_any(&self.inner_names, name, strength)
    }

    /// Is `name` the publisher's folder or key? "Acme" counts for
    /// "Acme Software Ltd.", as vendors shorten their own names.
    pub fn is_publisher(&self, name: &str) -> bool {
        let key = name_key(name);
        self.publisher_key
            .as_deref()
            .is_some_and(|own| key == own || (key.len() >= 4 && own.starts_with(&key)))
    }

    /// Does `text` (a command line, a path, registry data) point into the
    /// program's folders or at one of its executables by full path?
    pub fn references(&self, text: &str) -> bool {
        let Some(path) = embedded_path(text) else {
            return false;
        };
        self.owns_path(&path)
    }

    pub fn owns_path(&self, path: &Path) -> bool {
        self.install_dirs.iter().any(|dir| is_within(path, dir))
            && !self.others_dirs.iter().any(|dir| is_within(path, dir))
    }

    /// This identity for a scan: other installed programs living inside its
    /// folders are set apart, so no registration of theirs counts as ours.
    pub fn with_others(&self, others: &[crate::scan::OtherProgram]) -> Self {
        let mut scoped = self.clone();
        scoped.others_dirs = others
            .iter()
            .flat_map(|other| &other.dirs)
            .filter(|theirs| {
                self.install_dirs
                    .iter()
                    .any(|ours| is_within(theirs, ours) && !same(theirs, ours))
            })
            .cloned()
            .collect();
        scoped
    }
}

fn matches_any(names: &[String], name: &str, strength: Match) -> bool {
    let key = name_key(name);
    if is_generic(&key) {
        return false;
    }
    names.iter().any(|own| match strength {
        Match::Exact => key == *own,
        Match::Prefix => key == *own || (own.len() >= 4 && key.starts_with(own.as_str())),
        Match::Contains => key == *own || (own.len() >= 5 && key.contains(own.as_str())),
    })
}

/// Matching strictness, rising with the scan mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Exact,
    Prefix,
    Contains,
}

/// The host of a web address: `https://acme.example/widget` → `acme.example`.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url
        .trim()
        .split_once("://")
        .map_or(url.trim(), |(_, rest)| rest);
    let host = rest
        .split(['/', '?', '#'])
        .next()?
        .trim_start_matches("www.");
    let host = host.rsplit('@').next()?.split(':').next()?.to_lowercase();
    (host.contains('.') && !host.contains(' ')).then_some(host)
}

/// Keep a folder as an install dir only if it is a program's own folder, not
/// a shared place such as System32, the Installer cache or a root, and not
/// inside a folder already kept. Returns the folder when it was added.
fn push_dir(dirs: &mut Vec<PathBuf>, dir: PathBuf, folders: &Folders) -> Option<PathBuf> {
    // `C:\Program Files\Acme\` as installers often write it.
    let text = dir.to_string_lossy().into_owned();
    let dir = PathBuf::from(text.trim_end_matches(['\\', '/']));
    // Common Files itself and a publisher's folder in it are shared by a
    // suite; a product's own folder below that is its own.
    let common_shared = folders.common_files.iter().any(|common| {
        is_within(&dir, common)
            && dir
                .strip_prefix(common)
                .is_ok_and(|rest| rest.components().count() < 2)
    });
    let shared = common_shared
        || [
            folders.windows.as_path(),
            &folders.program_data.join("Package Cache"),
        ]
        .into_iter()
        .any(|place| is_within(&dir, place));
    let known = dirs.iter().any(|known| is_within(&dir, known));
    if !dir.is_absolute() || shared || is_root(&dir, folders) || known {
        return None;
    }
    dirs.retain(|known| !is_within(known, &dir));
    dirs.push(dir.clone());
    Some(dir)
}

/// A place programs are installed into, never a program's own folder: a
/// drive, Program Files, ProgramData, the AppData folders, the profile.
fn is_root(dir: &Path, folders: &Folders) -> bool {
    dir.parent().is_none()
        || folders.program_roots().iter().any(|root| same(dir, root))
        || same(dir, &folders.local_app_data.join("Programs"))
        || same(dir, &folders.user_profile)
}

/// Lower-case `.exe` names in `dir` and one level below, for App Paths and
/// Run entries that name the program by file.
fn executables(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let visit = |folder: &Path, recurse: bool, found: &mut Vec<String>| -> Vec<PathBuf> {
        let mut subdirs = Vec::new();
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() && recurse => subdirs.push(path),
                Ok(kind) if kind.is_file() => {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if name.ends_with(".exe") {
                        found.push(name);
                    }
                }
                _ => {}
            }
        }
        subdirs
    };
    for sub in visit(dir, true, &mut found) {
        visit(&sub, false, &mut found);
    }
    found
}

#[cfg(test)]
mod tests;
