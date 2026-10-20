//! A [`FileSpec`]'s filters (names, regular expressions, kind, age and
//! exclusions), compiled once per run and asked by the search for every
//! entry it meets.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use regex::{Regex, RegexBuilder};

use crate::model::{FileSpec, Kind, Search};
use crate::paths;
use crate::vars::{self, Vars};
use crate::wildcard::{Wildcard, WildcardSet};

#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("invalid regular expression {0:?}: {1}")]
    Regex(String, String),
    #[error("skipped: the exclusion {0:?} uses a variable this computer does not know")]
    Exclusion(String),
}

/// A spec's filters, compiled once per run.
pub struct Filter {
    pub(crate) regex: Option<Regex>,
    pub(crate) nregex: Option<Regex>,
    pub(crate) wholeregex: Option<Regex>,
    pub(crate) nwholeregex: Option<Regex>,
    pub(crate) names: Option<WildcardSet>,
    /// find.dirs: the folders it looks for, and the file beside them.
    pub(crate) dir_names: Option<WildcardSet>,
    pub(crate) beside: Option<WildcardSet>,
    pub(crate) kind: Option<Kind>,
    /// walk.all with a file-name filter: only matching files go, never the
    /// folders (some were already empty and are Windows' to keep).
    pub(crate) files_by_name: bool,
    pub(crate) older_than: Option<SystemTime>,
    pub(crate) exclude_roots: Vec<String>,
    pub(crate) exclude_patterns: Vec<Wildcard>,
    /// `FileSpec::keep_link`, in this platform's spelling.
    keep_link: Option<String>,
}

/// What a `keep_link` keeps among the items found in one folder: the link,
/// the item it names and everything in that item. Compared in the folder's
/// real spelling (every link on the way resolved), so a folder reached
/// through a link still matches what the link names.
pub(crate) struct Kept {
    folder: PathBuf,
    real_folder: PathBuf,
    link: String,
    target: String,
}

impl Kept {
    /// `item` in the folder's real spelling; `None` for an item not found
    /// in the folder, which is kept.
    fn real(&self, item: &Path) -> Option<String> {
        let rest = item.strip_prefix(&self.folder).ok()?;
        Some(paths::key(&self.real_folder.join(rest)))
    }

    /// Whether `item`, found in the folder or below it, must stay: it is
    /// the link, the item the link names or inside it, or a folder holding
    /// either.
    pub(crate) fn keeps(&self, item: &Path) -> bool {
        self.real(item).is_none_or(|key| {
            paths::is_within(&key, &self.target)
                || paths::is_within(&self.target, &key)
                || paths::is_within(&self.link, &key)
        })
    }

    /// Whether everything at or below `item` stays: it is the link, or the
    /// item the link names or inside it.
    pub(crate) fn keeps_all(&self, item: &Path) -> bool {
        self.real(item)
            .is_none_or(|key| paths::is_within(&key, &self.target) || key == self.link)
    }
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
            keep_link: spec.keep_link.as_deref().map(paths::native),
        })
    }

    /// What `keep_link` keeps in `folder`, read from the machine as it is
    /// now (a clean plans again, so an update since the preview is seen);
    /// `None` without a `keep_link`. `Err` says why nothing in the folder
    /// may go: the link is missing, is not a link, or names nothing in the
    /// folder, so which item is in use is not known.
    pub(crate) fn kept_in(&self, folder: &Path) -> Option<Result<Kept, String>> {
        let link = self.keep_link.as_deref()?;
        let kept = || -> Option<Kept> {
            let at = folder.join(link);
            if !fs::symlink_metadata(&at).ok()?.file_type().is_symlink() {
                return None;
            }
            let real_folder = fs::canonicalize(folder).ok()?;
            let target = paths::key(&fs::canonicalize(&at).ok()?);
            paths::is_within(&target, &paths::key(&real_folder)).then(|| Kept {
                folder: folder.to_path_buf(),
                link: paths::key(&real_folder.join(link)),
                target,
                real_folder,
            })
        };
        Some(kept().ok_or_else(|| {
            format!(
                "left alone: the link {link:?} is missing or names nothing in this folder, \
                 so the one in use is not known"
            )
        }))
    }

    pub(crate) fn excluded(&self, path: &Path) -> bool {
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
    pub(crate) fn path_ok(&self, path: &Path) -> bool {
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
    pub(crate) fn age_ok(&self, meta: &fs::Metadata) -> bool {
        self.older_than
            .is_none_or(|limit| meta.modified().is_ok_and(|modified| modified <= limit))
    }

    /// Name and age filters, applied to files (and to folders matched directly).
    pub(crate) fn name_ok(&self, path: &Path, meta: &fs::Metadata) -> bool {
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
