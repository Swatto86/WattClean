//! What a cleaner is: an application (or part of Windows) with options, each
//! option a list of actions. Definitions come from the built-in TOML files,
//! custom TOML or CleanerML files, and an imported winapp2.ini; all of them
//! end up as these types.

use serde::{Deserialize, Serialize};

/// Where a cleaner's definition came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    BuiltIn,
    Custom,
    Community,
}

/// The group a cleaner is listed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    System,
    Browsers,
    Applications,
    Developer,
    Games,
    DeepScan,
    Custom,
    Community,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleaner {
    /// Unique across the catalog; never contains a dot (option keys are
    /// `cleaner.option`).
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Category,
    pub source: Source,
    /// Executable names (lower case) that mean the application is running.
    pub running: Vec<String>,
    /// The cleaner is relevant when any of these exists; always when empty.
    pub detect: Vec<Detect>,
    pub options: Vec<CleanerOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detect {
    /// A file or folder (variables and wildcards allowed).
    Path(String),
    /// A registry key such as `HKCU\Software\7-Zip`.
    Registry(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanerOption {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Shown before the option is ticked, and again before cleaning.
    pub warning: Option<String>,
    /// Ticked in a fresh installation and by "Reset to defaults".
    pub default: bool,
    pub actions: Vec<Action>,
}

/// One thing an option does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Delete matching files (and, for the walk.all/top searches, folders).
    Delete(FileSpec),
    /// Empty matching files but keep them (log files an app keeps open).
    Truncate(FileSpec),
    /// Run a maintenance operation inside matching SQLite databases.
    Sqlite { files: FileSpec, op: SqliteOp },
    /// Remove a key from matching JSON files; `address` is `a/b/c`.
    Json { files: FileSpec, address: String },
    /// Remove a section, or one parameter of it, from matching INI files.
    Ini {
        files: FileSpec,
        section: String,
        parameter: Option<String>,
    },
    /// Delete a registry key (with its subkeys) or one value of it.
    Registry { key: String, value: Option<String> },
    /// Empty the Recycle Bin on every drive.
    RecycleBin,
    /// Clear the Windows clipboard.
    Clipboard,
    /// The files and folders listed under Settings > Custom paths.
    CustomPaths,
    /// Overwrite the free space of the drive holding the temporary folder
    /// (always run last, so space freed by the clean is wiped too).
    FreeDiskSpace,
}

/// How a path pattern is turned into files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Search {
    /// The path itself, if it exists.
    #[default]
    #[serde(rename = "file")]
    File,
    /// Wildcards (`*`, `?`) in any component.
    #[serde(rename = "glob")]
    Glob,
    /// Every file below the matched folders.
    #[serde(rename = "walk.files")]
    WalkFiles,
    /// Every file and folder below the matched folders.
    #[serde(rename = "walk.all")]
    WalkAll,
    /// As walk.all, then the matched folders themselves.
    #[serde(rename = "walk.top")]
    WalkTop,
    /// Folders named by `names` at any depth below the matched folders,
    /// each beside a file matching `beside` when it is given; each found
    /// folder is then searched as walk.top.
    #[serde(rename = "find.dirs")]
    FindDirs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    File,
    Dir,
}

/// Which files an action applies to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileSpec {
    /// May hold `%VARIABLES%` and wildcards.
    pub path: String,
    pub search: Search,
    /// Keep only names matching (case-insensitive regular expression).
    pub regex: Option<String>,
    /// Drop names matching.
    pub nregex: Option<String>,
    /// Keep only full paths matching.
    pub wholeregex: Option<String>,
    /// Drop full paths matching.
    pub nwholeregex: Option<String>,
    /// Keep only names matching these wildcards (`*.log;*.tmp`); for
    /// find.dirs, the names of the folders it looks for.
    pub names: Option<String>,
    /// For find.dirs: a file (wildcards) the found folder's parent must hold.
    pub beside: Option<String>,
    pub kind: Option<Kind>,
    /// Keep only items not modified for this many days.
    pub older_than_days: Option<u32>,
    /// Never touch items at or below these paths (variables and wildcards).
    pub exclude: Vec<String>,
}

impl FileSpec {
    pub fn new(path: impl Into<String>, search: Search) -> Self {
        Self {
            path: path.into(),
            search,
            ..Self::default()
        }
    }
}

/// Operations inside SQLite databases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SqliteOp {
    /// Rebuild the file to drop the space deleted rows left behind.
    Vacuum,
    /// Chromium's visited pages and search terms (History).
    ChromeHistory,
    /// Chromium's download list (History).
    ChromeDownloads,
    /// Chromium's form entries and addresses (Web Data).
    ChromeAutofill,
    /// Chromium's search engines added by sites (Web Data).
    ChromeKeywords,
    /// Firefox's visited pages, keeping bookmarks (places.sqlite).
    MozillaHistory,
    /// Firefox's download list (places.sqlite).
    MozillaDownloads,
}

impl SqliteOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Vacuum => "vacuum",
            Self::ChromeHistory => "history",
            Self::ChromeDownloads => "download history",
            Self::ChromeAutofill => "form history",
            Self::ChromeKeywords => "site search engines",
            Self::MozillaHistory => "history",
            Self::MozillaDownloads => "download history",
        }
    }
}

/// `cleaner.option`, the key a selection and the command line use.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OptionKey {
    pub cleaner: String,
    pub option: String,
}

impl OptionKey {
    pub fn new(cleaner: impl Into<String>, option: impl Into<String>) -> Self {
        Self {
            cleaner: cleaner.into(),
            option: option.into(),
        }
    }

    /// Parses `cleaner.option`; the cleaner part never holds a dot.
    pub fn parse(text: &str) -> Option<Self> {
        let (cleaner, option) = text.split_once('.')?;
        if cleaner.is_empty() || option.is_empty() {
            return None;
        }
        Some(Self::new(cleaner, option))
    }
}

impl std::fmt::Display for OptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.cleaner, self.option)
    }
}

/// Lower-case letters, digits and `_-`, for ids made from names.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('_');
            dash = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_keys_split_at_the_first_dot() {
        let key = OptionKey::parse("chrome.cache").unwrap();
        assert_eq!(key, OptionKey::new("chrome", "cache"));
        assert_eq!(key.to_string(), "chrome.cache");
        assert_eq!(OptionKey::parse("a.b.c"), Some(OptionKey::new("a", "b.c")));
        assert_eq!(OptionKey::parse("nodot"), None);
        assert_eq!(OptionKey::parse(".x"), None);
        assert_eq!(OptionKey::parse("x."), None);
    }

    #[test]
    fn slugs_are_ids() {
        assert_eq!(slug("Adobe Reader 9.x *"), "adobe_reader_9_x");
        assert_eq!(slug("  .NET Cache "), "net_cache");
        assert_eq!(slug("***"), "");
    }
}
