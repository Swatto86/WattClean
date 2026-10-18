//! A fake traced install: applies a scripted installer's changes to the
//! sandbox and fake registry, and reports them as a trace would.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::FakeSystem;
use crate::model::{RawValue, RegPath};
use crate::system::{Launch, System, Tracing};
use crate::trace::{Change, Trace};

/// What one fake installer does, matched by a fragment of its command line.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallEffect {
    pub command_contains: String,
    pub folders: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    /// Keys it creates, parents first, with their values.
    pub keys: Vec<(RegPath, Vec<RawValue>)>,
    /// Values it adds to keys that already exist.
    pub new_values: Vec<(RegPath, RawValue)>,
    /// Values that already exist and that it changes.
    pub changed_values: Vec<(RegPath, RawValue)>,
    /// Made and removed again during the install, as setups do with temp files.
    pub transient_file: Option<PathBuf>,
    pub transient_key: Option<RegPath>,
}

pub(super) fn trace(system: &FakeSystem, launch: &Launch) -> io::Result<Box<dyn Tracing>> {
    if !launch.program.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} does not exist", launch.program.display()),
        ));
    }
    let command = launch.display().to_lowercase();
    let effect = system
        .lock()
        .installs
        .iter()
        .find(|effect| command.contains(&effect.command_contains))
        .cloned()
        .unwrap_or_default();
    let mut changes = Vec::new();
    if let Some(file) = &effect.transient_file {
        write(file)?;
        std::fs::remove_file(file)?;
        changes.push(Change::FileCreated { path: file.clone() });
        changes.push(Change::FileDeleted { path: file.clone() });
    }
    if let Some(key) = &effect.transient_key {
        system.create_key(key)?;
        system.delete_key_tree(key)?;
        changes.push(Change::KeyCreated { key: key.clone() });
        changes.push(Change::KeyDeleted { key: key.clone() });
    }
    for folder in &effect.folders {
        std::fs::create_dir_all(folder)?;
        changes.push(Change::FolderCreated {
            path: folder.clone(),
        });
    }
    for file in &effect.files {
        write(file)?;
        changes.push(Change::FileCreated { path: file.clone() });
    }
    for (key, values) in &effect.keys {
        system.create_key(key)?;
        changes.push(Change::KeyCreated { key: key.clone() });
        for value in values {
            system.write_value(key, value)?;
            changes.push(Change::ValueSet {
                key: key.clone(),
                name: value.name.clone(),
                existed: false,
            });
        }
    }
    for (key, value, existed) in effect
        .new_values
        .iter()
        .map(|(key, value)| (key, value, false))
        .chain(
            effect
                .changed_values
                .iter()
                .map(|(key, value)| (key, value, true)),
        )
    {
        system.write_value(key, value)?;
        changes.push(Change::ValueSet {
            key: key.clone(),
            name: value.name.clone(),
            existed,
        });
    }
    let trace = Trace {
        changes,
        processes: vec![launch.program.display().to_string()],
        events_lost: 0,
        unnamed: 0,
    };
    Ok(Box::new(FakeTracing {
        polls_left: 2,
        trace,
    }))
}

fn write(file: &std::path::Path) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, b"fake")
}

struct FakeTracing {
    polls_left: u32,
    trace: Trace,
}

impl Tracing for FakeTracing {
    fn finished(&mut self) -> io::Result<bool> {
        self.polls_left = self.polls_left.saturating_sub(1);
        Ok(self.polls_left == 0)
    }

    fn finish(self: Box<Self>) -> io::Result<Trace> {
        Ok(self.trace)
    }
}
