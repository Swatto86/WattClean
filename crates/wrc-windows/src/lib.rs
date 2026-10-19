//! The real machine behind `wrc_core::System`.
#![cfg(windows)]

mod accounts;
mod drive;
mod hives;
mod launch;
mod notify;
mod privilege;
mod registry;
mod restore;
mod security;
mod session;
mod shell;
mod tasks;
#[cfg(test)]
mod tests;
mod toast;
mod trace;

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS};
use wrc_core::keyname::long_name;
use wrc_core::model::{RawValue, RegPath};
use wrc_core::system::{Account, DriveKind, KeyWatch};
use wrc_core::trace::RegistryTrace;
use wrc_core::{Hive, System};

pub use launch::accept_launch_messages;
pub use privilege::{
    ELEVATION_REQUIRED, enable_privilege, is_elevated, relaunch_elevated, wait_for_exit,
};
pub use trace::handle_addresses;

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
    /// to the account's hive (opening it first when it is signed out).
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

    fn drive_kind(&self, path: &Path) -> DriveKind {
        drive::drive_kind(path)
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

    /// Registry Editor can show a signed-in account's keys (under its SID)
    /// but not those of a hive this app opened privately.
    fn open_in_regedit(&self, key: &RegPath) -> io::Result<()> {
        let shown = if key.account_part().is_some() {
            let (root, real) = self.locate(key)?;
            if !Arc::ptr_eq(&root, &self.users) {
                return Err(io::Error::other(
                    "Registry Editor cannot show this account's registry while it is signed out.",
                ));
            }
            real
        } else {
            key.clone()
        };
        shell::open_in_regedit(&long_name(&shown))
    }

    fn restart_explorer(&self) -> io::Result<()> {
        shell::restart_explorer()
    }

    fn open_url(&self, url: &str) -> io::Result<()> {
        shell::open_url(url)
    }

    fn sign_out(&self) -> io::Result<()> {
        session::sign_out()
    }

    fn restart_pc(&self) -> io::Result<()> {
        session::restart()
    }

    fn save_hive(&self, key: &RegPath, file: &Path) -> io::Result<()> {
        hives::save(key, file, privilege::user_sid().as_deref())
    }

    fn watch_key(&self, key: &RegPath) -> io::Result<Box<dyn KeyWatch>> {
        let (root, key) = self.locate(key)?;
        notify::Notify::open(&root, &key).map(|watch| Box::new(watch) as Box<dyn KeyWatch>)
    }

    fn key_security(&self, key: &RegPath) -> io::Result<String> {
        let (root, key) = self.locate(key)?;
        security::read(&root, &key.path)
    }

    fn set_key_security(&self, key: &RegPath, sddl: &str) -> io::Result<()> {
        let (root, key) = self.locate(key)?;
        security::write(&root, &key.path, sddl)
    }

    fn schedule_task(&self, name: &str, xml: &str) -> io::Result<()> {
        tasks::schedule(name, xml)
    }

    fn delete_task(&self, name: &str) -> io::Result<()> {
        tasks::delete(name)
    }

    fn task_exists(&self, name: &str) -> io::Result<bool> {
        tasks::exists(name)
    }

    fn notify(&self, title: &str, body: &str) -> io::Result<()> {
        toast::show(title, body)
    }

    fn trace_registry(&self) -> io::Result<Box<dyn RegistryTrace>> {
        Ok(Box::new(trace::start(privilege::user_sid())?))
    }

    fn take_ownership(&self, key: &RegPath, subkeys: bool) -> io::Result<()> {
        let user = privilege::user_sid()
            .ok_or_else(|| io::Error::other("Windows did not say who this account is"))?;
        let (root, key) = self.locate(key)?;
        security::take(&root, &key.path, &user, subkeys)
    }
}
