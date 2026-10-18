//! Other accounts' registries. A signed-in account's hives are loaded by
//! Windows under `HKEY_USERS\<SID>`; a signed-out account's live in its
//! profile as `NTUSER.DAT` and `UsrClass.dat`. Those are opened directly as
//! application hives (`RegLoadAppKey`: no mount, no privilege) for as long
//! as a scan, removal or restore needs them, then closed. A hive with
//! unsaved changes cannot be opened that way; it is then mounted under
//! `HKEY_USERS\Account-<name>` (`RegLoadKey`, which applies the changes) and
//! unmounted after, with stale mounts from a crash unmounted at start-up,
//! because a mount left behind breaks that account's next sign-in.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use windows_sys::Win32::System::Registry::{
    HKEY_USERS as HKEY_USERS_RAW, RegLoadKeyW, RegUnLoadKeyW,
};
use winreg::RegKey;
use winreg::enums::{HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, KEY_WOW64_64KEY};
use wrc_core::model::{ACCOUNT_PREFIX, CLASSES_SUFFIX};
use wrc_core::system::Account;

use crate::SharedKey;
use crate::privilege::{enable_privilege, wide};

const PROFILE_LIST: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";

/// What Windows knows of one account's profile.
#[derive(Debug, Clone)]
pub struct Profile {
    pub sid: String,
    pub account: Account,
}

/// Every normal account with a profile folder, except the current one.
pub fn profiles(current_sid: Option<&str>) -> Vec<Profile> {
    let list = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(PROFILE_LIST, KEY_READ | KEY_WOW64_64KEY);
    let Ok(list) = list else {
        return Vec::new();
    };
    let users = RegKey::predef(HKEY_USERS);
    let mut found = Vec::new();
    for sid in list.enum_keys().flatten() {
        // S-1-5-21-… are real accounts; the rest are Windows' own services.
        if !sid.starts_with("S-1-5-21-") || current_sid == Some(sid.as_str()) {
            continue;
        }
        let Ok(entry) = list.open_subkey_with_flags(&sid, KEY_READ | KEY_WOW64_64KEY) else {
            continue;
        };
        let Ok(image) = entry.get_value::<String, _>("ProfileImagePath") else {
            continue;
        };
        let profile = PathBuf::from(expand_env(&image));
        let Some(name) = profile
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        if !profile.join("NTUSER.DAT").is_file() {
            continue;
        }
        let signed_in = users
            .open_subkey_with_flags(&sid, KEY_READ | KEY_WOW64_64KEY)
            .is_ok();
        found.push(Profile {
            sid,
            account: Account {
                name,
                profile,
                signed_in,
            },
        });
    }
    found.sort_by(|a, b| a.account.name.cmp(&b.account.name));
    found
}

fn expand_env(text: &str) -> String {
    let mut out = text.to_owned();
    for (name, value) in std::env::vars() {
        let token = format!("%{name}%");
        if let Some(at) = out.to_lowercase().find(&token.to_lowercase()) {
            out.replace_range(at..at + token.len(), &value);
        }
    }
    out
}

/// Where a key really is: under `HKEY_USERS`, or in a hive opened here.
pub enum Resolved {
    /// `HKEY_USERS` with the full path: a signed-in account's SID, a hive
    /// mounted here, or something else mounted there.
    Users(String),
    /// A hive file opened by this process, with the path inside it.
    Hive(Arc<SharedKey>, String),
}

/// Hive files this process opened, by mount name, and hives it mounted.
#[derive(Default)]
pub struct Mounts {
    open: Mutex<HashMap<String, Arc<SharedKey>>>,
    mounted: Mutex<HashSet<String>>,
}

impl Mounts {
    pub fn resolve(&self, profiles: &[Profile], mount: &str, rest: &str) -> io::Result<Resolved> {
        let parsed = mount.strip_prefix(ACCOUNT_PREFIX).map(|name| {
            match name.strip_suffix(CLASSES_SUFFIX) {
                Some(account) => (account, true),
                None => (name, false),
            }
        });
        let Some((account, classes)) = parsed else {
            return Ok(Resolved::Users(join(mount, rest)));
        };
        let Some(profile) = profiles
            .iter()
            .find(|profile| profile.account.name == account)
        else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no account called {account}"),
            ));
        };
        if profile.account.signed_in {
            let suffix = if classes { CLASSES_SUFFIX } else { "" };
            return Ok(Resolved::Users(join(
                &format!("{}{suffix}", profile.sid),
                rest,
            )));
        }
        let file = hive_file(&profile.account.profile, classes);
        match self.open(mount, &file) {
            Ok(hive) => Ok(Resolved::Hive(hive, rest.to_owned())),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                self.mount(mount, &file).map_err(|mount_error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{error}; {mount_error}"),
                    )
                })?;
                Ok(Resolved::Users(join(mount, rest)))
            }
            Err(error) => Err(error),
        }
    }

    fn open(&self, mount: &str, file: &Path) -> io::Result<Arc<SharedKey>> {
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(hive) = open.get(mount) {
            return Ok(Arc::clone(hive));
        }
        if !file.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} does not exist", file.display()),
            ));
        }
        // Not locked to this process: Windows must still be able to sign the
        // account in; that sign-in then fails our later writes, not theirs.
        let hive = RegKey::load_app_key(file, false).map_err(|error| {
            let kind = match error.raw_os_error() {
                // Access denied, sharing violation: in use or no rights.
                Some(5 | 32) => io::ErrorKind::PermissionDenied,
                // ERROR_BADDB: unsaved changes only a mount can apply.
                Some(1009) => io::ErrorKind::InvalidData,
                _ => error.kind(),
            };
            io::Error::new(kind, format!("could not open {}: {error}", file.display()))
        })?;
        let hive = Arc::new(SharedKey(hive));
        open.insert(mount.to_owned(), Arc::clone(&hive));
        Ok(hive)
    }

    /// Mount a hive under `HKEY_USERS\<mount>`, applying its unsaved changes.
    fn mount(&self, mount: &str, file: &Path) -> io::Result<()> {
        let mut mounted = self
            .mounted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if mounted.contains(mount) {
            return Ok(());
        }
        enable_privilege("SeBackupPrivilege")?;
        enable_privilege("SeRestorePrivilege")?;
        let name = wide(OsStr::new(mount));
        let path = wide(file.as_os_str());
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let status = unsafe { RegLoadKeyW(HKEY_USERS_RAW, name.as_ptr(), path.as_ptr()) };
        if status != 0 {
            let error = io::Error::from_raw_os_error(i32::try_from(status).unwrap_or(-1));
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("mounting {} failed: {error}", file.display()),
            ));
        }
        mounted.insert(mount.to_owned());
        Ok(())
    }

    /// Close every hive opened here and unmount every hive mounted here.
    /// The first unmount failure is returned after the rest were attempted.
    pub fn release(&self) -> io::Result<()> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        let mut mounted = self
            .mounted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut first_error = None;
        for mount in mounted.drain() {
            if let Err(error) = unmount(&mount) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for Mounts {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

fn unmount(mount: &str) -> io::Result<()> {
    let name = wide(OsStr::new(mount));
    // SAFETY: the name is NUL-terminated and outlives the call.
    let status = unsafe { RegUnLoadKeyW(HKEY_USERS_RAW, name.as_ptr()) };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(-1),
        ));
    }
    Ok(())
}

/// Unmount `Account-*` hives a previous run left behind (a crash while one
/// was mounted). Errors are ignored: nothing better can be done at start-up.
pub fn unmount_stale() {
    let users = RegKey::predef(HKEY_USERS);
    for mount in users.enum_keys().flatten() {
        if mount.starts_with(ACCOUNT_PREFIX) {
            let _ = enable_privilege("SeRestorePrivilege");
            let _ = unmount(&mount);
        }
    }
}

pub fn hive_file(profile: &Path, classes: bool) -> PathBuf {
    if classes {
        profile.join(r"AppData\Local\Microsoft\Windows\UsrClass.dat")
    } else {
        profile.join("NTUSER.DAT")
    }
}

fn join(mount: &str, rest: &str) -> String {
    if rest.is_empty() {
        mount.to_owned()
    } else {
        format!("{mount}\\{rest}")
    }
}
