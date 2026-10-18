//! The port to the machine: registry, drives, restore points and the shell.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::{RawValue, RegPath, find_value};
use crate::trace::RegistryTrace;

/// Another account on this PC, whose registry can be read under
/// `HKU\Account-<name>` ([`RegPath::account_root`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    pub profile: PathBuf,
    /// Signed in now: its hives are loaded by Windows and read in place.
    pub signed_in: bool,
}

/// What kind of drive a path is on, which decides whether a missing file
/// there is really gone or only unplugged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveKind {
    /// A fixed disk: a missing file is gone.
    Fixed,
    /// USB sticks, memory cards, optical discs: may simply be unplugged.
    Removable,
    /// A network share: checking could hang, and it may be offline.
    Network,
    /// No such drive now (a disk that is not attached).
    Missing,
}

/// A watched key: reports when anything beneath it changes.
pub trait KeyWatch: Send {
    /// Wait up to `timeout` for a change; true when one happened.
    fn wait(&mut self, timeout: Duration) -> io::Result<bool>;
}

/// Everything the engine needs from Windows. Registry calls address the
/// 64-bit view; a missing key is `io::ErrorKind::NotFound`. Keys under
/// `HKU\Account-<name>` reach that account's hives, opened from its profile
/// when it is not signed in; a hive that cannot be opened (in use, no
/// administrator rights) answers `io::ErrorKind::PermissionDenied`.
pub trait System: Send + Sync {
    fn subkeys(&self, key: &RegPath) -> io::Result<Vec<String>>;
    fn values(&self, key: &RegPath) -> io::Result<Vec<RawValue>>;
    fn create_key(&self, key: &RegPath) -> io::Result<()>;
    /// Create the key if needed and write the value.
    fn write_value(&self, key: &RegPath, value: &RawValue) -> io::Result<()>;
    /// Delete the key with everything beneath it.
    fn delete_key_tree(&self, key: &RegPath) -> io::Result<()>;
    fn delete_value(&self, key: &RegPath, name: &str) -> io::Result<()>;

    fn env_var(&self, name: &str) -> Option<String>;
    fn is_elevated(&self) -> bool;
    fn create_restore_point(&self, description: &str) -> io::Result<()>;
    fn drive_kind(&self, path: &Path) -> DriveKind;
    /// Every other account with a profile on this PC.
    fn other_accounts(&self) -> Vec<Account>;
    /// Close every account hive opened on demand. Called after each scan,
    /// clean and restore: a hive left open breaks that account's sign-in.
    fn release_account_hives(&self) -> io::Result<()>;
    /// Start Registry Editor showing `key`.
    fn open_in_regedit(&self, key: &RegPath) -> io::Result<()>;
    /// End File Explorer and start it again, so shell settings take effect.
    fn restart_explorer(&self) -> io::Result<()>;
    /// Open a web address in the person's browser.
    fn open_url(&self, url: &str) -> io::Result<()>;
    /// Sign out of Windows, as the Start menu's Sign out does.
    fn sign_out(&self) -> io::Result<()>;
    /// Restart Windows.
    fn restart_pc(&self) -> io::Result<()>;
    /// Save a whole hive (`HKLM\SOFTWARE`, `HKLM\SYSTEM`, `HKU\.DEFAULT`,
    /// `HKCU`, `HKCU\Software\Classes`) to a new file. Administrator rights.
    fn save_hive(&self, key: &RegPath, file: &Path) -> io::Result<()>;
    fn watch_key(&self, key: &RegPath) -> io::Result<Box<dyn KeyWatch>>;
    /// The key's owner, group and permissions, as SDDL text.
    fn key_security(&self, key: &RegPath) -> io::Result<String>;
    /// Put back owner, group and permissions read with [`Self::key_security`].
    fn set_key_security(&self, key: &RegPath, sddl: &str) -> io::Result<()>;
    /// Make this account the owner of `key` (and of every key beneath it
    /// with `subkeys`) and give it full control, replacing the permissions.
    /// Needs administrator rights for keys owned by someone else.
    fn take_ownership(&self, key: &RegPath, subkeys: bool) -> io::Result<()>;
    /// Create or replace a Task Scheduler task from its XML definition.
    fn schedule_task(&self, name: &str, xml: &str) -> io::Result<()>;
    /// Delete a Task Scheduler task; one that does not exist is fine.
    fn delete_task(&self, name: &str) -> io::Result<()>;
    fn task_exists(&self, name: &str) -> io::Result<bool>;
    /// Start tracing registry writes with the process behind each. Needs
    /// administrator rights; tracing stops when the result is dropped.
    fn trace_registry(&self) -> io::Result<Box<dyn RegistryTrace>>;
    /// Show a Windows notification under the app's name.
    fn notify(&self, title: &str, body: &str) -> io::Result<()>;
}

/// The subkeys of `key`, or none when it does not exist or cannot be read.
pub fn subkeys_or_empty(system: &dyn System, key: &RegPath) -> Vec<String> {
    system.subkeys(key).unwrap_or_default()
}

pub fn values_or_empty(system: &dyn System, key: &RegPath) -> Vec<RawValue> {
    system.values(key).unwrap_or_default()
}

pub fn key_exists(system: &dyn System, key: &RegPath) -> bool {
    system.subkeys(key).is_ok()
}

/// A text value, trimmed; `None` when absent, not text, or blank.
pub fn text_value(values: &[RawValue], name: &str) -> Option<String> {
    find_value(values, name)
        .and_then(RawValue::as_text)
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}
