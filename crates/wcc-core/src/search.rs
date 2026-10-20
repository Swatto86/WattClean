//! Turning a [`FileSpec`] into the files and folders it names.
//!
//! Symbolic links and junctions are never followed: a link found by a walk
//! is listed as itself (removing it leaves its target alone), and a wildcard
//! never descends through one. A folder is listed only when everything in it
//! is listed too, so removing it after its contents is what the preview said.

use std::fs;
use std::path::{Path, PathBuf};

pub use crate::filter::{Filter, SpecError};
use crate::model::{FileSpec, Kind, Search};
use crate::paths::{self, Guard};
use crate::vars::{self, Vars};
use crate::wildcard::glob_report;

/// Walks stop this deep, far below any real application's folders.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub kind: Kind,
    pub size: u64,
}

/// A folder that could not be read, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Findings {
    pub found: Vec<Found>,
    pub problems: Vec<Problem>,
}

/// Context shared by one search.
pub struct Searcher<'a> {
    pub guard: &'a Guard,
    pub cancelled: &'a dyn Fn() -> bool,
}

impl Searcher<'_> {
    /// Everything `spec` names, after its filters and the guard.
    pub fn find(&self, spec: &FileSpec, filter: &Filter, vars: &dyn Vars, out: &mut Findings) {
        if spec.literal {
            self.find_literal(Path::new(&spec.path), spec.search, filter, out);
            return;
        }
        for pattern in vars::expand(&spec.path, vars) {
            let roots = match spec.search {
                Search::File => vec![PathBuf::from(paths::native(&pattern))],
                _ => {
                    let (roots, refused) = glob_report(&pattern);
                    out.problems
                        .extend(refused.into_iter().map(|(path, error)| Problem {
                            message: describe(&error),
                            path,
                        }));
                    roots
                }
            };
            for root in roots {
                if (self.cancelled)() {
                    return;
                }
                let folder = root.parent().unwrap_or(&root);
                let kept = match filter.kept_in(folder) {
                    // Said once for the folder, however many items it holds.
                    Some(Err(message)) => {
                        let problem = Problem {
                            path: folder.to_path_buf(),
                            message,
                        };
                        if !out.problems.contains(&problem) {
                            out.problems.push(problem);
                        }
                        continue;
                    }
                    Some(Ok(kept)) if kept.keeps_all(&root) => continue,
                    kept => kept.and_then(Result::ok),
                };
                let start = out.found.len();
                self.search_root(&root, spec.search, filter, out);
                // What this root holds that the link keeps (a folder
                // holding the item in use).
                if let Some(kept) = kept {
                    let found = out.found.split_off(start);
                    out.found
                        .extend(found.into_iter().filter(|f| !kept.keeps(&f.path)));
                }
            }
        }
    }

    /// What a path a person chose names, taken exactly as written: no
    /// variables, wildcards or separator changes, so only the file or
    /// folder they picked is found (on Linux and macOS a name may hold `%`,
    /// `*`, `?` and `\`). A link is still only the link (`paths::chosen`).
    pub fn find_literal(&self, path: &Path, search: Search, filter: &Filter, out: &mut Findings) {
        if !(self.cancelled)() {
            self.search_root(&paths::chosen(path), search, filter, out);
        }
    }

    fn search_root(&self, root: &Path, search: Search, filter: &Filter, out: &mut Findings) {
        let meta = match fs::symlink_metadata(root) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                out.problems.push(Problem {
                    path: root.to_path_buf(),
                    message: describe(&error),
                });
                return;
            }
            Err(_) => return,
        };
        if paths::is_special(&meta) {
            return;
        }
        let is_dir = meta.is_dir() && !meta.file_type().is_symlink();
        match search {
            Search::File | Search::Glob => {
                let kind = paths::kind_of(&meta);
                if filter.kind.is_some_and(|k| k != kind)
                    || !filter.path_ok(root)
                    || !filter.name_ok(root, &meta)
                {
                    return;
                }
                // A link's size is its own text, nothing that would be freed.
                let size = if kind == Kind::File && !meta.file_type().is_symlink() {
                    meta.len()
                } else {
                    0
                };
                self.push(out, root, kind, size);
            }
            Search::WalkFiles | Search::WalkAll | Search::WalkTop if is_dir => {
                let files_only = search == Search::WalkFiles;
                let complete = self.walk(root, 0, files_only, filter, out);
                if search == Search::WalkTop
                    && complete
                    && filter.kind != Some(Kind::File)
                    && filter.path_ok(root)
                {
                    self.push(out, root, Kind::Dir, 0);
                }
            }
            Search::FindDirs => {
                if is_dir {
                    self.find_dirs(root, 0, filter, out);
                }
            }
            // A walk over a file: the file itself, as BleachBit does.
            _ if !is_dir
                && filter.kind != Some(Kind::Dir)
                && filter.path_ok(root)
                && filter.name_ok(root, &meta) =>
            {
                self.push(out, root, Kind::File, meta.len());
            }
            _ => {}
        }
    }

    /// Lists a folder's contents, deepest first. Returns whether every item
    /// in it was listed, so the folder itself may go too.
    fn walk(
        &self,
        dir: &Path,
        depth: usize,
        files_only: bool,
        filter: &Filter,
        out: &mut Findings,
    ) -> bool {
        if depth > MAX_DEPTH
            || (self.cancelled)()
            || self.guard.is_kept(dir)
            || (depth > 0 && filter.excluded(dir))
        {
            return false;
        }
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                out.problems.push(Problem {
                    path: dir.to_path_buf(),
                    message: describe(&error),
                });
                return false;
            }
        };
        // By name, so a preview lists in the same order on every platform
        // (Windows returns names sorted, Linux in no particular order).
        let mut entries: Vec<_> = entries.collect();
        entries.sort_by_key(|entry| entry.as_ref().ok().map(fs::DirEntry::file_name));
        let mut complete = true;
        for entry in entries {
            let Ok(entry) = entry else {
                complete = false;
                continue;
            };
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                complete = false;
                continue;
            };
            let link = meta.file_type().is_symlink();
            if paths::is_special(&meta) {
                complete = false;
                continue;
            }
            if meta.is_dir() && !link {
                let inner = self.walk(&path, depth + 1, files_only, filter, out);
                if files_only
                    || filter.files_by_name
                    || !inner
                    || filter.kind == Some(Kind::File)
                    || !filter.path_ok(&path)
                    || !filter.age_ok(&meta)
                {
                    complete = false;
                    continue;
                }
                if !self.push(out, &path, Kind::Dir, 0) {
                    complete = false;
                }
            } else {
                // A link to a folder is removed like a folder (the link only).
                let kind = paths::kind_of(&meta);
                let wanted = match kind {
                    Kind::File => filter.kind != Some(Kind::Dir),
                    Kind::Dir => !files_only && filter.kind != Some(Kind::File),
                };
                if !wanted || !filter.path_ok(&path) || !filter.name_ok(&path, &meta) {
                    complete = false;
                    continue;
                }
                let size = if kind == Kind::File && !link {
                    meta.len()
                } else {
                    0
                };
                if !self.push(out, &path, kind, size) {
                    complete = false;
                }
            }
        }
        complete
    }

    /// find.dirs below `dir`: a wanted folder is searched as walk.top, any
    /// other folder is looked through in turn.
    fn find_dirs(&self, dir: &Path, depth: usize, filter: &Filter, out: &mut Findings) {
        if depth > MAX_DEPTH || (self.cancelled)() || self.guard.is_kept(dir) {
            return;
        }
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                out.problems.push(Problem {
                    path: dir.to_path_buf(),
                    message: describe(&error),
                });
                return;
            }
        };
        let mut folders = Vec::new();
        let mut marked = false;
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if file_type.is_dir() && !file_type.is_symlink() {
                folders.push((entry.path(), name));
            } else if filter.beside.as_ref().is_some_and(|w| w.matches(&name)) {
                marked = true;
            }
        }
        folders.sort();
        for (path, name) in folders {
            if filter.excluded(&path) {
                continue;
            }
            let wanted = filter.dir_names.as_ref().is_some_and(|w| w.matches(&name))
                && (marked || filter.beside.is_none());
            if wanted {
                self.search_root(&path, Search::WalkTop, filter, out);
            } else {
                self.find_dirs(&path, depth + 1, filter, out);
            }
        }
    }

    /// Adds an item unless the guard refuses it; says whether it was added.
    fn push(&self, out: &mut Findings, path: &Path, kind: Kind, size: u64) -> bool {
        if self.guard.check(path).is_err() {
            return false;
        }
        out.found.push(Found {
            path: path.to_path_buf(),
            kind,
            size,
        });
        true
    }
}

/// A short reason for an I/O failure that a person can act on.
pub fn describe(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        // ERROR_ACCESS_DENIED; on Linux and macOS 5 is an input/output error.
        Some(5) if cfg!(windows) => {
            "access denied (administrator rights may be needed)".to_string()
        }
        // ERROR_SHARING_VIOLATION, which sqlite.rs also reports for a busy
        // database on every platform.
        Some(32) => "in use by another program".to_string(),
        // EPERM: macOS's privacy protection, not file permissions.
        Some(1) if cfg!(target_os = "macos") => "macOS did not allow this (a protected item: \
             giving WattClean Full Disk Access in System Settings › Privacy & Security may help)"
            .to_string(),
        _ => error.to_string(),
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "search_keep_tests.rs"]
mod keep_tests;
