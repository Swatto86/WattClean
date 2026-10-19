//! The single operations a run is made of: planned from an option's actions
//! (what a preview lists) and carried out one by one (what a clean does).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::edit;
use crate::model::{Action, FileSpec, Kind, Search, SqliteOp};
use crate::paths::{self, Guard};
use crate::report::{Item, ItemKind};
use crate::search::{Filter, Findings, Problem, Searcher};
use crate::sqlite;
use crate::system::System;
use crate::tasks::{Listed, Task};
use crate::vars::Vars;

/// A file or folder from Settings > Custom paths.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomPath {
    pub path: String,
    pub folder: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    DeleteFile {
        path: PathBuf,
        size: u64,
    },
    DeleteDir {
        path: PathBuf,
    },
    Truncate {
        path: PathBuf,
        size: u64,
    },
    Sqlite {
        path: PathBuf,
        op: SqliteOp,
        size: u64,
    },
    Json {
        path: PathBuf,
        address: String,
    },
    Ini {
        path: PathBuf,
        section: String,
        parameter: Option<String>,
    },
    RegistryKey {
        key: String,
    },
    RegistryValue {
        key: String,
        name: String,
    },
    FreeSpace {
        folder: PathBuf,
        free: u64,
    },
    RecycleBin {
        items: u64,
        bytes: u64,
    },
    Clipboard,
    /// A maintenance command (`tasks::TASKS`), with what its listing step
    /// found and the size of what it frees, when that can be measured.
    Command {
        task: &'static Task,
        program: PathBuf,
        listed: Vec<Listed>,
        /// What the measured folders held when planned.
        size: u64,
        /// What the task's own dry run says it would free, when it has one.
        estimate: Option<u64>,
    },
}

impl Op {
    /// The item a preview lists for this operation.
    pub fn item(&self) -> Item {
        let (kind, path, size, note) = match self {
            Self::DeleteFile { path, size } => (ItemKind::File, show(path), *size, None),
            Self::DeleteDir { path } => (ItemKind::Folder, show(path), 0, None),
            Self::Truncate { path, size } => (
                ItemKind::Truncate,
                show(path),
                *size,
                Some("emptied".to_string()),
            ),
            Self::Sqlite { path, op, .. } => (
                ItemKind::Database,
                show(path),
                0,
                Some(op.label().to_string()),
            ),
            Self::Json { path, address } => (ItemKind::Json, show(path), 0, Some(address.clone())),
            Self::Ini {
                path,
                section,
                parameter,
            } => (
                ItemKind::Ini,
                show(path),
                0,
                Some(match parameter {
                    Some(p) => format!("[{section}] {p}"),
                    None => format!("[{section}]"),
                }),
            ),
            Self::RegistryKey { key } => (ItemKind::Registry, key.clone(), 0, None),
            Self::RegistryValue { key, name } => (
                ItemKind::Registry,
                format!(r"{key}\{name}"),
                0,
                Some("value".to_string()),
            ),
            Self::RecycleBin { items, bytes } => (
                ItemKind::RecycleBin,
                TRASH.to_string(),
                *bytes,
                Some(format!(
                    "{items} item{}",
                    if *items == 1 { "" } else { "s" }
                )),
            ),
            Self::Clipboard => (ItemKind::Clipboard, "Clipboard".to_string(), 0, None),
            Self::Command {
                task,
                listed,
                size,
                estimate,
                ..
            } => (
                ItemKind::Command,
                task.command_line(),
                estimate.unwrap_or(*size),
                crate::command::note(task, listed),
            ),
            Self::FreeSpace { folder, free } => (
                ItemKind::FreeSpace,
                show(folder),
                0,
                Some(format!("about {} MB of free space overwritten", free >> 20)),
            ),
        };
        Item {
            kind,
            path,
            size,
            note,
        }
    }

    /// The file this operation touches, for de-duplication across options.
    fn file_key(&self) -> Option<String> {
        match self {
            Self::DeleteFile { path, .. }
            | Self::DeleteDir { path }
            | Self::Truncate { path, .. } => Some(paths::key(path)),
            _ => None,
        }
    }
}

/// What the system calls the place deleted files wait.
const TRASH: &str = if cfg!(windows) {
    "Recycle Bin"
} else {
    "Trash"
};

fn show(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// What planning needs from the run.
pub struct Planner<'a> {
    pub system: &'a dyn System,
    pub guard: &'a Guard,
    pub custom_paths: &'a [CustomPath],
    pub now: std::time::SystemTime,
    pub cancelled: &'a dyn Fn() -> bool,
}

impl Planner<'_> {
    /// The operations for one action, leaving out files already planned.
    pub fn plan(&self, action: &Action, seen: &mut HashSet<String>) -> (Vec<Op>, Vec<Problem>) {
        let lookup = |name: &str| self.system.var(name);
        let mut ops = Vec::new();
        let mut problems = Vec::new();
        match action {
            Action::Delete(spec) => self.files(spec, &lookup, &mut problems, |found| {
                ops.push(match found.kind {
                    Kind::File => Op::DeleteFile {
                        path: found.path,
                        size: found.size,
                    },
                    Kind::Dir => Op::DeleteDir { path: found.path },
                });
            }),
            Action::Truncate(spec) => self.files(spec, &lookup, &mut problems, |found| {
                if found.kind == Kind::File && found.size > 0 {
                    ops.push(Op::Truncate {
                        path: found.path,
                        size: found.size,
                    });
                }
            }),
            Action::Sqlite { files, op } => self.files(files, &lookup, &mut problems, |found| {
                if found.kind == Kind::File && sqlite::is_database(&found.path) {
                    ops.push(Op::Sqlite {
                        path: found.path,
                        op: *op,
                        size: found.size,
                    });
                }
            }),
            Action::Json { files, address } => self.files(files, &lookup, &mut problems, |found| {
                if found.kind == Kind::File && edit::json_has(&found.path, address) {
                    ops.push(Op::Json {
                        path: found.path,
                        address: address.clone(),
                    });
                }
            }),
            Action::Ini {
                files,
                section,
                parameter,
            } => self.files(files, &lookup, &mut problems, |found| {
                if found.kind == Kind::File
                    && edit::ini_has(&found.path, section, parameter.as_deref())
                {
                    ops.push(Op::Ini {
                        path: found.path,
                        section: section.clone(),
                        parameter: parameter.clone(),
                    });
                }
            }),
            Action::Registry { key, value } => ops.extend(self.registry(key, value.as_deref())),
            Action::RecycleBin => match self.system.recycle_bin() {
                Ok((items, bytes)) if items > 0 => ops.push(Op::RecycleBin { items, bytes }),
                Ok(_) => {}
                Err(error) => problems.push(Problem {
                    path: PathBuf::from(TRASH),
                    message: error.to_string(),
                }),
            },
            Action::Clipboard => {
                if self.system.clipboard_has_data() {
                    ops.push(Op::Clipboard);
                }
            }
            Action::FreeDiskSpace => {
                if let Some(op) = self.free_space() {
                    ops.push(op);
                }
            }
            Action::Command(task) => match self.command(task) {
                Ok(Some(op)) => ops.push(op),
                Ok(None) => {}
                Err(problem) => problems.push(problem),
            },
            Action::CustomPaths => {
                for custom in self.custom_paths {
                    let search = if custom.folder {
                        Search::WalkTop
                    } else {
                        Search::File
                    };
                    let spec = FileSpec::new(custom.path.clone(), search);
                    let (more, issues) = self.plan(&Action::Delete(spec), seen);
                    ops.extend(more);
                    problems.extend(issues);
                }
                // Each inner plan has already left out what was seen.
                return (ops, problems);
            }
        }
        ops.retain(|op| op.file_key().is_none_or(|key| seen.insert(key)));
        (ops, problems)
    }

    /// A registry key or value, if it is there to delete.
    fn registry(&self, key: &str, value: Option<&str>) -> Option<Op> {
        let registry = self.system.registry();
        match value {
            Some(name) if registry.value_exists(key, name) => Some(Op::RegistryValue {
                key: key.to_string(),
                name: name.to_string(),
            }),
            None if registry.key_exists(key) => Some(Op::RegistryKey {
                key: key.to_string(),
            }),
            _ => None,
        }
    }

    /// The free space of the drive holding the temporary folder.
    fn free_space(&self) -> Option<Op> {
        let temp = self.system.var("TEMP").into_iter().next()?;
        let drive = self.system.drive_of(Path::new(&temp))?;
        let folder = self.system.wipe_folder(&drive);
        let free = self.system.free_space(&folder).unwrap_or(drive.free);
        Some(Op::FreeSpace { folder, free })
    }

    fn files(
        &self,
        spec: &FileSpec,
        vars: &dyn Vars,
        problems: &mut Vec<Problem>,
        mut each: impl FnMut(crate::search::Found),
    ) {
        let filter = match Filter::new(spec, vars, self.now) {
            Ok(filter) => filter,
            Err(error) => {
                problems.push(Problem {
                    path: PathBuf::from(&spec.path),
                    message: error.to_string(),
                });
                return;
            }
        };
        let searcher = Searcher {
            guard: self.guard,
            cancelled: self.cancelled,
        };
        let mut findings = Findings::default();
        searcher.find(spec, &filter, vars, &mut findings);
        problems.extend(findings.problems);
        findings.found.into_iter().for_each(&mut each);
    }
}
