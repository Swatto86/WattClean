//! The fake machine's answers to the engine's port.

use std::io;
use std::path::Path;

use super::{FakeRunning, FakeSystem, children, ensure_key, install, not_found};
use crate::model::{RawValue, RegPath};
use crate::system::{Account, Launch, Running, System, Tracing};

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
        state
            .keys
            .get(&key.folded())
            .map(|entry| entry.values.clone())
            .ok_or_else(|| not_found(key))
    }

    fn key_modified(&self, key: &RegPath) -> io::Result<u64> {
        let state = self.lock();
        state
            .keys
            .get(&key.folded())
            .map(|entry| entry.modified)
            .ok_or_else(|| not_found(key))
    }

    fn create_key(&self, key: &RegPath) -> io::Result<()> {
        self.mutate(|state| {
            ensure_key(state, key);
            Ok(())
        })
    }

    fn write_value(&self, key: &RegPath, value: &RawValue) -> io::Result<()> {
        self.mutate(|state| {
            ensure_key(state, key);
            let entry = state
                .keys
                .get_mut(&key.folded())
                .ok_or_else(|| not_found(key))?;
            entry
                .values
                .retain(|old| !old.name.eq_ignore_ascii_case(&value.name));
            entry.values.push(value.clone());
            Ok(())
        })
    }

    fn delete_key_tree(&self, key: &RegPath) -> io::Result<()> {
        self.mutate(|state| {
            let folded = key.folded();
            if !state.keys.contains_key(&folded) {
                return Err(not_found(key));
            }
            let prefix = format!("{folded}\\");
            state
                .keys
                .retain(|name, _| *name != folded && !name.starts_with(&prefix));
            Ok(())
        })
    }

    fn delete_value(&self, key: &RegPath, name: &str) -> io::Result<()> {
        self.mutate(|state| {
            let entry = state
                .keys
                .get_mut(&key.folded())
                .ok_or_else(|| not_found(key))?;
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

    fn launch(&self, launch: &Launch) -> io::Result<Box<dyn Running>> {
        if !launch.program.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} does not exist", launch.program.display()),
            ));
        }
        let command = launch.display().to_lowercase();
        let effect = self.mutate(|state| {
            state.launches.push(launch.clone());
            let effect = state
                .effects
                .iter()
                .find(|effect| command.contains(&effect.command_contains))
                .cloned();
            if let Some(effect) = &effect {
                for key in &effect.delete_keys {
                    let folded = key.folded();
                    let prefix = format!("{folded}\\");
                    state
                        .keys
                        .retain(|name, _| *name != folded && !name.starts_with(&prefix));
                }
            }
            Ok(effect)
        })?;
        for path in effect.iter().flat_map(|effect| &effect.delete_paths) {
            if path.is_dir() {
                std::fs::remove_dir_all(path)?;
            } else if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        Ok(Box::new(FakeRunning { polls_left: 2 }))
    }

    fn delete_on_reboot(&self, path: &Path) -> io::Result<()> {
        self.mutate(|state| {
            state.reboot_deletes.push(path.to_path_buf());
            Ok(())
        })
    }

    fn trace_install(&self, launch: &Launch) -> io::Result<Box<dyn Tracing>> {
        install::trace(self, launch)
    }

    /// Every profile folder under `Users` but the current one and Windows'
    /// own; an account is "signed in" when its hive was seeded as loaded.
    fn other_accounts(&self) -> Vec<Account> {
        let users = self.root.join("Users");
        let current = self.env_var("USERPROFILE").unwrap_or_default();
        let mut accounts: Vec<Account> = std::fs::read_dir(&users)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let special = ["Public", "Default", "Default User", "All Users"];
                let profile = entry.path();
                if special.contains(&name.as_str())
                    || crate::paths::same(&profile, Path::new(&current))
                {
                    return None;
                }
                Some(Account {
                    name,
                    profile,
                    signed_in: false,
                })
            })
            .collect();
        accounts.sort_by(|a, b| a.name.cmp(&b.name));
        accounts
    }

    fn release_account_hives(&self) -> io::Result<()> {
        self.mutate(|state| {
            state.hive_releases += 1;
            Ok(())
        })
    }
}
