//! What a preview or a clean found or did, per option and in total, and the
//! progress reported while it runs.

use serde::Serialize;

use crate::model::OptionKey;

/// The window lists at most this many items per option; the rest are counted.
pub const ITEM_CAP: usize = 400;
/// Problems kept per option.
pub const PROBLEM_CAP: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Preview,
    Clean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemKind {
    File,
    Folder,
    Truncate,
    Registry,
    Database,
    Json,
    Ini,
    RecycleBin,
    Clipboard,
    FreeSpace,
    /// A package manager's (or other tool's) maintenance command.
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub kind: ItemKind,
    pub path: String,
    pub size: u64,
    /// A word on what happens or happened, such as "deleted at restart".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemLine {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub registry: u64,
    /// Database, JSON, INI, Recycle Bin and clipboard operations.
    pub other: u64,
    pub problems: u64,
    /// Files skipped because a program had them open.
    pub in_use: u64,
}

impl Counts {
    pub fn add_item(&mut self, item: &Item) {
        match item.kind {
            ItemKind::File | ItemKind::Truncate => self.files += 1,
            ItemKind::Folder => self.folders += 1,
            ItemKind::Registry => self.registry += 1,
            _ => self.other += 1,
        }
        self.bytes += item.size;
    }

    pub fn absorb(&mut self, other: &Counts) {
        self.files += other.files;
        self.folders += other.folders;
        self.bytes += other.bytes;
        self.registry += other.registry;
        self.other += other.other;
        self.problems += other.problems;
        self.in_use += other.in_use;
    }

    pub fn items(&self) -> u64 {
        self.files + self.folders + self.registry + self.other
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionReport {
    pub key: String,
    pub cleaner: String,
    pub option: String,
    pub counts: Counts,
    pub items: Vec<Item>,
    /// Items beyond [`ITEM_CAP`].
    pub more: u64,
    pub problems: Vec<ProblemLine>,
}

impl OptionReport {
    pub fn new(key: &OptionKey, cleaner: &str, option: &str) -> Self {
        Self {
            key: key.to_string(),
            cleaner: cleaner.to_string(),
            option: option.to_string(),
            counts: Counts::default(),
            items: Vec::new(),
            more: 0,
            problems: Vec::new(),
        }
    }

    pub fn push_item(&mut self, item: Item) {
        self.counts.add_item(&item);
        if self.items.len() < ITEM_CAP {
            self.items.push(item);
        } else {
            self.more += 1;
        }
    }

    pub fn push_problem(
        &mut self,
        path: impl Into<String>,
        message: impl Into<String>,
        in_use: bool,
    ) {
        self.counts.problems += 1;
        if in_use {
            self.counts.in_use += 1;
        }
        if self.problems.len() < PROBLEM_CAP {
            self.problems.push(ProblemLine {
                path: path.into(),
                message: message.into(),
            });
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub mode: Mode,
    pub options: Vec<OptionReport>,
    pub totals: Counts,
    pub cancelled: bool,
    pub elapsed_ms: u64,
    /// Selected programs that were running. On Windows the files they hold
    /// open are skipped (in use); on macOS and Linux, which would delete
    /// them from under the program, their options were left alone, with a
    /// problem saying so.
    pub running: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub mode: Mode,
    /// `Cleaner › Option` being worked on.
    pub label: String,
    /// Options finished, of `total`.
    pub done: usize,
    pub total: usize,
    pub items: u64,
    pub bytes: u64,
}

/// Receives progress and every item; says when to stop.
pub trait Sink: Sync {
    fn progress(&self, _progress: &Progress) {}
    /// Every item found (preview) or done (clean), including those past the
    /// window's cap; the command line prints them.
    fn item(&self, _key: &OptionKey, _item: &Item) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// A sink that ignores everything and never cancels.
pub struct Quiet;

impl Sink for Quiet {}
