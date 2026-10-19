//! A [`FileSpec`]'s filters (names, regular expressions, kind, age and
//! exclusions), compiled once per run and asked by the search for every
//! entry it meets.

use std::fs;
use std::path::Path;
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
    #[error("skipped: the exclusion {0:?} uses a variable this PC does not know")]
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
