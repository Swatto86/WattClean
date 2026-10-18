//! What an install trace records, in the names people use: drive paths and
//! HKLM/HKCU keys, in the order the changes happened.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::RegPath;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Change {
    FileCreated {
        path: PathBuf,
    },
    FolderCreated {
        path: PathBuf,
    },
    /// An existing file opened to be replaced or truncated.
    FileReplaced {
        path: PathBuf,
    },
    FileDeleted {
        path: PathBuf,
    },
    FileRenamed {
        path: PathBuf,
    },
    KeyCreated {
        key: RegPath,
    },
    KeyDeleted {
        key: RegPath,
    },
    /// `existed` is true when the value had data before: a change, not a creation.
    ValueSet {
        key: RegPath,
        name: String,
        existed: bool,
    },
    ValueDeleted {
        key: RegPath,
        name: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    pub changes: Vec<Change>,
    /// Image paths of the processes the installer started.
    pub processes: Vec<String>,
    /// Events Windows dropped; above zero, the record may be incomplete.
    pub events_lost: u32,
    /// Registry changes whose key could not be named.
    pub unnamed: u32,
}
