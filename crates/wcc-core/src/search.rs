//! Turning a [`FileSpec`] into the files and folders it names.
//!
//! Symbolic links and junctions are never followed: a link found by a walk
//! is listed as itself (removing it leaves its target alone), and a wildcard
//! never descends through one. A folder is listed only when everything in it
//! is listed too, so removing it after its contents is what the preview said.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use regex::{Regex, RegexBuilder};

use crate::model::{FileSpec, Kind, Search};
use crate::paths::{self, Guard};
use crate::vars::{self, Vars};
use crate::wildcard::{Wildcard, WildcardSet, glob};

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

#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("invalid regular expression {0:?}: {1}")]
    Regex(String, String),
    #[error("skipped: the exclusion {0:?} uses a variable this PC does not know")]
    Exclusion(String),
}

/// A spec's filters, compiled once per run.
pub struct Filter {
    regex: Option<Regex>,
    nregex: Option<Regex>,
    wholeregex: Option<Regex>,
    nwholeregex: Option<Regex>,
    names: Option<WildcardSet>,
    /// find.dirs: the folders it looks for, and the file beside them.
    dir_names: Option<WildcardSet>,
    beside: Option<WildcardSet>,
    kind: Option<Kind>,
    /// walk.all with a file-name filter: only matching files go, never the
    /// folders (some were already empty and are Windows' to keep).
    files_by_name: bool,
    older_than: Option<SystemTime>,
    exclude_roots: Vec<String>,
    exclude_patterns: Vec<Wildcard>,
}

fn compile(pattern: Option<&String>) -> Result<Option<Regex>, SpecError> {
    pattern
        .map(|p| {
            RegexBuilder::new(p)
                .case_insensitive(true)
                .build()
                .map_err(|e| SpecError::Regex(p.clone(), e.to_string()))
        })
        .transpose()
}

impl Filter {
    pub fn new(spec: &FileSpec, vars: &dyn Vars, now: SystemTime) -> Result<Self, SpecError> {
        let mut exclude_roots = Vec::new();
        let mut exclude_patterns = Vec::new();
        for pattern in &spec.exclude {
            let Some(expansions) = vars::expand_exclusion(pattern, vars) else {
                return Err(SpecError::Exclusion(pattern.clone()));
            };
            for expanded in expansions {
                if Wildcard::has_wildcards(&expanded) {
                    let key = paths::key(Path::new(&expanded));
                    // `\*\` spans any number of folders, none included.
                    if key.contains(r"\*\") {
                        exclude_patterns.push(Wildcard::new(&key.replace(r"\*\", r"\")));
                    }
                    exclude_patterns.push(Wildcard::new(&key));
                } else {
                    exclude_roots.push(paths::key(Path::new(&expanded)));
                }
            }
        }
        // find.dirs names the folders it finds, not what is in them.
        let names = spec.names.as_deref().map(WildcardSet::new);
        let (names, dir_names) = if spec.search == Search::FindDirs {
            (None, names)
        } else {
            (names, None)
        };
        let files_by_name = spec.search == Search::WalkAll
            && (spec.names.is_some() || spec.regex.is_some() || spec.nregex.is_some());
        Ok(Self {
            regex: compile(spec.regex.as_ref())?,
            nregex: compile(spec.nregex.as_ref())?,
            wholeregex: compile(spec.wholeregex.as_ref())?,
            nwholeregex: compile(spec.nwholeregex.as_ref())?,
            names,
            dir_names,
            beside: spec.beside.as_deref().map(WildcardSet::new),
            kind: spec.kind,
            files_by_name,
            older_than: spec
                .older_than_days
                .and_then(|days| now.checked_sub(Duration::from_secs(u64::from(days) * 86_400))),
            exclude_roots,
            exclude_patterns,
        })
    }

    fn excluded(&self, path: &Path) -> bool {
        if self.exclude_roots.is_empty() && self.exclude_patterns.is_empty() {
            return false;
        }
        let key = paths::key(path);
        self.exclude_roots
            .iter()
            .any(|root| paths::is_within(&key, root))
            || self.exclude_patterns.iter().any(|w| w.matches(&key))
    }

    /// Whole-path filters, applied to files and folders alike.
    fn path_ok(&self, path: &Path) -> bool {
        let text = path.to_string_lossy();
        if self.wholeregex.as_ref().is_some_and(|r| !r.is_match(&text)) {
            return false;
        }
        if self.nwholeregex.as_ref().is_some_and(|r| r.is_match(&text)) {
            return false;
        }
        !self.excluded(path)
    }

    /// The age filter alone, for folders a walk reaches.
    fn age_ok(&self, meta: &fs::Metadata) -> bool {
        self.older_than
            .is_none_or(|limit| meta.modified().is_ok_and(|modified| modified <= limit))
    }

    /// Name and age filters, applied to files (and to folders matched directly).
    fn name_ok(&self, path: &Path, meta: &fs::Metadata) -> bool {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        if self.regex.as_ref().is_some_and(|r| !r.is_match(&name)) {
            return false;
        }
        if self.nregex.as_ref().is_some_and(|r| r.is_match(&name)) {
            return false;
        }
        if self.names.as_ref().is_some_and(|w| !w.matches(&name)) {
            return false;
        }
        if let Some(limit) = self.older_than {
            match meta.modified() {
                Ok(modified) if modified <= limit => {}
                _ => return false,
            }
        }
        true
    }
}

/// Context shared by one search.
pub struct Searcher<'a> {
    pub guard: &'a Guard,
    pub cancelled: &'a dyn Fn() -> bool,
}

impl Searcher<'_> {
    /// Everything `spec` names, after its filters and the guard.
    pub fn find(&self, spec: &FileSpec, filter: &Filter, vars: &dyn Vars, out: &mut Findings) {
        for pattern in vars::expand(&spec.path, vars) {
            let roots = match spec.search {
                Search::File => vec![PathBuf::from(&pattern)],
                _ => glob(&pattern),
            };
            for root in roots {
                if (self.cancelled)() {
                    return;
                }
                self.search_root(&root, spec.search, filter, out);
            }
        }
    }

    fn search_root(&self, root: &Path, search: Search, filter: &Filter, out: &mut Findings) {
        let Ok(meta) = fs::symlink_metadata(root) else {
            return;
        };
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
                self.push(
                    out,
                    root,
                    kind,
                    if kind == Kind::File { meta.len() } else { 0 },
                );
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
        Some(5) => "access denied (administrator rights may be needed)".to_string(),
        Some(32) => "in use by another program".to_string(),
        _ => error.to_string(),
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
