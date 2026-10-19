//! The fake machine's answers to the engine's port.

use std::io;
use std::path::{Component, Path, Prefix};
use std::sync::Arc;

use super::trace::FakeTrace;
use super::watch::FakeWatch;
use super::{DEFAULT_SECURITY, FakeKey, FakeSystem, OWNED, children, ensure_key, not_found};
use crate::model::{RawValue, RegPath};
use crate::system::{Account, DriveKind, KeyWatch, System};
use crate::trace::{RegistryTrace, WriteOp};

impl System for FakeSystem {
    fn subkeys(&self, key: &RegPath) -> io::Result<Vec<String>> {
        let state = self.lock();
        if !key.path.is_empty() && !state.keys.contains_key(&key.folded()) {
            return Err(not_found(key));
        }
        Ok(children(&state, key)
            .map(|entry| entry.path.name().to_owned())
            .collect())
    }

    fn values(&self, key: &RegPath) -> io::Result<Vec<RawValue>> {
        let state = self.lock();
        if key.path.is_empty() {
            return Ok(Vec::new());
        }
        state
            .keys
            .get(&key.folded())
            .map(|entry| entry.values.clone())
            .ok_or_else(|| not_found(key))
    }

    fn create_key(&self, key: &RegPath) -> io::Result<()> {
        self.mutate(|state| {
            let parent_protected = key
                .parent()
                .and_then(|parent| state.keys.get(&parent.folded()))
                .is_some_and(FakeKey::protected);
            let exists = state.keys.contains_key(&key.folded());
            if parent_protected && !exists {
                return Err(denied(key));
            }
            ensure_key(state, key);
            if !exists {
                state.traced(WriteOp::KeyCreated, key, None);
            }
            Ok(())
        })
    }

    fn write_value(&self, key: &RegPath, value: &RawValue) -> io::Result<()> {
        self.mutate(|state| {
            if state
                .keys
                .get(&key.folded())
                .is_some_and(FakeKey::protected)
            {
                return Err(denied(key));
            }
            if !state.keys.contains_key(&key.folded()) {
                ensure_key(state, key);
                state.traced(WriteOp::KeyCreated, key, None);
            }
            let entry = state
                .keys
                .get_mut(&key.folded())
                .ok_or_else(|| not_found(key))?;
            let old = entry
                .values
                .iter()
                .find(|old| old.name.eq_ignore_ascii_case(&value.name))
                .cloned();
            entry
                .values
                .retain(|old| !old.name.eq_ignore_ascii_case(&value.name));
            entry.values.push(value.clone());
            state.traced_set(key, value, old.as_ref());
            Ok(())
        })
    }

    fn delete_key_tree(&self, key: &RegPath) -> io::Result<()> {
        self.mutate(|state| {
            let folded = key.folded();
            if !state.keys.contains_key(&folded) {
                return Err(not_found(key));
            }
            if state.keys.get(&folded).is_some_and(FakeKey::protected) {
                return Err(denied(key));
            }
            // As RegDeleteTree does, subkeys go one by one, deepest first,
            // until a protected one stops it: what went before stays gone.
            let prefix = format!("{folded}\\");
            let inside: Vec<String> = state
                .keys
                .keys()
                .rev()
                .filter(|name| **name == folded || name.starts_with(&prefix))
                .cloned()
                .collect();
            for name in inside {
                let Some(entry) = state.keys.get(&name) else {
                    continue;
                };
                if entry.protected() {
                    return Err(denied(key));
                }
                let path = entry.path.clone();
                state.keys.remove(&name);
                state.traced(WriteOp::KeyDeleted, &path, None);
            }
            Ok(())
        })
    }

    fn delete_value(&self, key: &RegPath, name: &str) -> io::Result<()> {
        self.mutate(|state| {
            let entry = state
                .keys
                .get_mut(&key.folded())
                .ok_or_else(|| not_found(key))?;
            if entry.protected() {
                return Err(denied(key));
            }
            let before = entry.values.len();
            entry
                .values
                .retain(|value| !value.name.eq_ignore_ascii_case(name));
            if entry.values.len() == before {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no value {name}"),
                ));
            }
            state.traced(WriteOp::ValueDeleted, key, Some(name));
            Ok(())
        })
    }

    fn env_var(&self, name: &str) -> Option<String> {
        self.lock().env.get(&name.to_uppercase()).cloned()
    }

    fn is_elevated(&self) -> bool {
        self.lock().elevated
    }

    fn create_restore_point(&self, description: &str) -> io::Result<()> {
        self.mutate(|state| {
            state.restore_points.push(description.to_owned());
            Ok(())
        })
    }

    /// The sandbox's own drive is a fixed disk; every other letter is a
    /// drive that is not attached, whatever the real machine has.
    fn drive_kind(&self, path: &Path) -> DriveKind {
        if drive_letter(path).is_some() && drive_letter(path) == drive_letter(&self.root) {
            DriveKind::Fixed
        } else {
            DriveKind::Missing
        }
    }

    /// Every profile folder under the sandbox's `Users` but the current one.
    fn other_accounts(&self) -> Vec<Account> {
        let users = self.root.join("Users");
        let current = self.env_var("USERPROFILE").unwrap_or_default();
        let mut accounts: Vec<Account> = std::fs::read_dir(&users)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter(|entry| !crate::paths::is_within(&entry.path(), Path::new(&current)))
            .map(|entry| Account {
                name: entry.file_name().to_string_lossy().into_owned(),
                profile: entry.path(),
                signed_in: false,
            })
            .collect();
        accounts.sort_by(|a, b| a.name.cmp(&b.name));
        accounts
    }

    fn release_account_hives(&self) -> io::Result<()> {
        self.lock().hive_releases += 1;
        Ok(())
    }

    fn open_in_regedit(&self, key: &RegPath) -> io::Result<()> {
        self.mutate(|state| {
            state.opened.push(key.display());
            Ok(())
        })
    }

    fn restart_explorer(&self) -> io::Result<()> {
        self.mutate(|state| {
            state.explorer_restarts += 1;
            Ok(())
        })
    }

    fn open_url(&self, url: &str) -> io::Result<()> {
        self.mutate(|state| {
            state.opened_urls.push(url.to_owned());
            Ok(())
        })
    }

    fn sign_out(&self) -> io::Result<()> {
        self.mutate(|state| {
            state.sign_outs += 1;
            Ok(())
        })
    }

    fn restart_pc(&self) -> io::Result<()> {
        self.mutate(|state| {
            state.pc_restarts += 1;
            Ok(())
        })
    }

    /// The hive's keys as JSON; a file already there is refused, as
    /// Windows refuses it.
    fn save_hive(&self, key: &RegPath, file: &Path) -> io::Result<()> {
        let state = self.lock();
        if !state.elevated {
            return Err(denied(key));
        }
        if file.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the file exists",
            ));
        }
        let prefix = format!("{}\\", key.folded());
        let keys: Vec<&FakeKey> = state
            .keys
            .iter()
            .filter(|(name, _)| {
                key.path.is_empty() || **name == key.folded() || name.starts_with(&prefix)
            })
            .filter(|(_, entry)| entry.path.hive == key.hive)
            .map(|(_, entry)| entry)
            .collect();
        let text = serde_json::to_string(&keys).map_err(io::Error::other)?;
        std::fs::write(file, text)
    }

    fn key_security(&self, key: &RegPath) -> io::Result<String> {
        let state = self.lock();
        let entry = state
            .keys
            .get(&key.folded())
            .ok_or_else(|| not_found(key))?;
        Ok(entry
            .security
            .clone()
            .unwrap_or_else(|| DEFAULT_SECURITY.to_owned()))
    }

    /// Changing another owner's key needs administrator rights.
    fn set_key_security(&self, key: &RegPath, sddl: &str) -> io::Result<()> {
        self.mutate(|state| {
            if !state.elevated {
                return Err(denied(key));
            }
            let entry = state
                .keys
                .get_mut(&key.folded())
                .ok_or_else(|| not_found(key))?;
            entry.security = Some(sddl.to_owned());
            Ok(())
        })
    }

    fn take_ownership(&self, key: &RegPath, subkeys: bool) -> io::Result<()> {
        self.mutate(|state| {
            if !state.elevated {
                return Err(denied(key));
            }
            let folded = key.folded();
            if !state.keys.contains_key(&folded) {
                return Err(not_found(key));
            }
            let prefix = format!("{folded}\\");
            for (name, entry) in &mut state.keys {
                if *name == folded || (subkeys && name.starts_with(&prefix)) {
                    entry.security = Some(OWNED.to_owned());
                }
            }
            Ok(())
        })
    }

    fn schedule_task(&self, name: &str, xml: &str) -> io::Result<()> {
        self.mutate(|state| {
            state.tasks.insert(name.to_owned(), xml.to_owned());
            Ok(())
        })
    }

    /// A task made with the highest rights is Administrators': others
    /// cannot delete it.
    fn delete_task(&self, name: &str) -> io::Result<()> {
        self.mutate(|state| {
            let highest = state
                .tasks
                .get(name)
                .is_some_and(|xml| xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
            if highest && !state.elevated {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Access is denied.",
                ));
            }
            state.tasks.remove(name);
            Ok(())
        })
    }

    fn task_exists(&self, name: &str) -> io::Result<bool> {
        Ok(self.lock().tasks.contains_key(name))
    }

    fn notify(&self, title: &str, body: &str) -> io::Result<()> {
        self.mutate(|state| {
            state
                .notifications
                .push((title.to_owned(), body.to_owned()));
            Ok(())
        })
    }

    fn trace_registry(&self) -> io::Result<Box<dyn RegistryTrace>> {
        Ok(Box::new(FakeTrace::start(&self.state)?))
    }

    fn watch_key(&self, key: &RegPath) -> io::Result<Box<dyn KeyWatch>> {
        if !key.path.is_empty() && !self.has_key(key) {
            return Err(not_found(key));
        }
        let seen = self.lock().generation;
        Ok(Box::new(FakeWatch {
            state: Arc::clone(&self.state),
            seen,
        }))
    }
}

fn denied(key: &RegPath) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("access to {} is denied", key.display()),
    )
}

fn drive_letter(path: &Path) -> Option<u8> {
    match path.components().next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                Some(letter.to_ascii_uppercase())
            }
            _ => None,
        },
        _ => None,
    }
}
