//! The real machine behind `wu_core::System`.
#![cfg(windows)]

mod accounts;
mod privilege;
mod process;
mod registry;
mod restore;
#[cfg(test)]
mod tests;
pub mod trace;
pub mod trace_event;
mod traced;

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS};
use wu_core::model::{RawValue, RegPath};
use wu_core::system::Account;
use wu_core::{Hive, Launch, Running, System, Tracing};

pub use privilege::{ELEVATION_REQUIRED, enable_privilege, is_elevated, relaunch_elevated};

/// A registry key handle shared between threads. Win32 registry functions
/// are safe to call on one handle from several threads at once, and a
/// handle is not tied to the thread that opened it.
pub struct SharedKey(RegKey);

// SAFETY: see the type's documentation; the wrapper holds nothing else.
unsafe impl Send for SharedKey {}
// SAFETY: as above.
unsafe impl Sync for SharedKey {}

impl std::ops::Deref for SharedKey {
    type Target = RegKey;
    fn deref(&self) -> &RegKey {
        &self.0
    }
}

pub struct WindowsSystem {
    local_machine: Arc<SharedKey>,
    current_user: Arc<SharedKey>,
    users: Arc<SharedKey>,
    elevated: bool,
    /// Other accounts' profiles, read when first needed and on each listing.
    profiles: Mutex<Option<Vec<accounts::Profile>>>,
    mounts: accounts::Mounts,
}

impl WindowsSystem {
    pub fn new() -> Self {
        let elevated = is_elevated();
        if elevated {
            accounts::unmount_stale();
        }
        Self {
            local_machine: Arc::new(SharedKey(RegKey::predef(HKEY_LOCAL_MACHINE))),
            current_user: Arc::new(SharedKey(RegKey::predef(HKEY_CURRENT_USER))),
            users: Arc::new(SharedKey(RegKey::predef(HKEY_USERS))),
            elevated,
            profiles: Mutex::new(None),
            mounts: accounts::Mounts::default(),
        }
    }

    /// Registry roots taken from private hive files, for tests that must
    /// not touch the machine's real registry.
    #[cfg(test)]
    fn with_hives(local_machine: RegKey, current_user: RegKey) -> Self {
        Self {
            local_machine: Arc::new(SharedKey(local_machine)),
            current_user: Arc::new(SharedKey(current_user)),
            users: Arc::new(SharedKey(RegKey::predef(HKEY_USERS))),
            elevated: false,
            profiles: Mutex::new(Some(Vec::new())),
            mounts: accounts::Mounts::default(),
        }
    }

    /// Accounts known in advance, for tests with a hive file of their own.
    #[cfg(test)]
    fn with_profiles(profiles: Vec<accounts::Profile>) -> Self {
        let system = Self::new();
        *system
            .profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(profiles);
        system
    }

    fn profiles(&self, refresh: bool) -> Vec<accounts::Profile> {
        let mut cached = self
            .profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if refresh || cached.is_none() {
            *cached = Some(accounts::profiles(privilege::user_sid().as_deref()));
        }
        cached.clone().unwrap_or_default()
    }

    /// The root key and path a key is really at. An `HKU\Account-…` key maps
    /// to the account's loaded hive (loading it first when it is signed out).
    fn locate(&self, key: &RegPath) -> io::Result<(Arc<SharedKey>, RegPath)> {
        match key.hive {
            Hive::LocalMachine => Ok((Arc::clone(&self.local_machine), key.clone())),
            Hive::CurrentUser => Ok((Arc::clone(&self.current_user), key.clone())),
            Hive::Users => {
                let (mount, rest) = key.path.split_once('\\').unwrap_or((&key.path, ""));
                match self.mounts.resolve(&self.profiles(false), mount, rest)? {
                    accounts::Resolved::Users(path) => {
                        Ok((Arc::clone(&self.users), RegPath::new(Hive::Users, &path)))
                    }
                    accounts::Resolved::Hive(hive, rest) => {
                        Ok((hive, RegPath::new(Hive::Users, &rest)))
                    }
                }
            }
        }
    }
}

impl Default for WindowsSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl System for WindowsSystem {
    fn subkeys(&self, key: &RegPath) -> io::Result<Vec<String>> {
        let (root, key) = self.locate(key)?;
        registry::subkeys(&root, &key)
    }

    fn values(&self, key: &RegPath) -> io::Result<Vec<RawValue>> {
        let (root, key) = self.locate(key)?;
        registry::values(&root, &key)
    }

    fn key_modified(&self, key: &RegPath) -> io::Result<u64> {
        let (root, key) = self.locate(key)?;
        registry::modified(&root, &key)
    }

    fn create_key(&self, key: &RegPath) -> io::Result<()> {
        let (root, key) = self.locate(key)?;
        registry::create(&root, &key).map(|_| ())
    }

    fn write_value(&self, key: &RegPath, value: &RawValue) -> io::Result<()> {
        let (root, key) = self.locate(key)?;
        registry::write_value(&root, &key, value)
    }

    fn delete_key_tree(&self, key: &RegPath) -> io::Result<()> {
        let (root, key) = self.locate(key)?;
        registry::delete_tree(&root, &key)
    }

    fn delete_value(&self, key: &RegPath, name: &str) -> io::Result<()> {
        let (root, key) = self.locate(key)?;
        registry::delete_value(&root, &key, name)
    }

    fn env_var(&self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        std::env::var(name).ok()
    }

    fn is_elevated(&self) -> bool {
        self.elevated
    }

    fn create_restore_point(&self, description: &str) -> io::Result<()> {
        restore::create_restore_point(description)
    }

    fn launch(&self, launch: &Launch) -> io::Result<Box<dyn Running>> {
        process::launch(launch).map(|running| Box::new(running) as Box<dyn Running>)
    }

    fn delete_on_reboot(&self, path: &Path) -> io::Result<()> {
        restore::delete_on_reboot(path)
    }

    fn trace_install(&self, launch: &Launch) -> io::Result<Box<dyn Tracing>> {
        if !self.elevated {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "tracing an install needs administrator rights",
            ));
        }
        traced::start(launch)
    }

    fn other_accounts(&self) -> Vec<Account> {
        self.profiles(true)
            .into_iter()
            .map(|profile| profile.account)
            .collect()
    }

    fn release_account_hives(&self) -> io::Result<()> {
        self.mounts.release()
    }
}
