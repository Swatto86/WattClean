//! The port to the machine: registry, processes and privileges.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{RawValue, RegPath, find_value};

/// A program to start: the executable and its argument text exactly as the
/// uninstall entry wrote it (Windows programs parse their own command line).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    pub program: PathBuf,
    pub args: String,
}

impl Launch {
    pub fn display(&self) -> String {
        let program = self.program.display();
        if self.args.is_empty() {
            format!("\"{program}\"")
        } else {
            format!("\"{program}\" {}", self.args)
        }
    }
}

/// A started uninstaller, tracked with every process it starts.
pub trait Running: Send {
    /// True once the uninstaller and everything it started have exited.
    fn finished(&mut self) -> io::Result<bool>;
}

/// An installer running under an install trace.
pub trait Tracing: Send {
    /// True once the installer and everything it started have exited.
    fn finished(&mut self) -> io::Result<bool>;
    /// Stop tracing and return what was recorded.
    fn finish(self: Box<Self>) -> io::Result<crate::trace::Trace>;
}

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

/// Everything the engine needs from Windows. Registry calls address the
/// 64-bit view; a missing key is `io::ErrorKind::NotFound`. Keys under
/// `HKU\Account-<name>` reach that account's hives, loaded from its profile
/// when it is not signed in; a hive that cannot be loaded (its file in use,
/// no administrator rights) answers `io::ErrorKind::PermissionDenied`.
pub trait System: Send + Sync {
    fn subkeys(&self, key: &RegPath) -> io::Result<Vec<String>>;
    fn values(&self, key: &RegPath) -> io::Result<Vec<RawValue>>;
    /// The key's last write time, in seconds since the Unix epoch.
    fn key_modified(&self, key: &RegPath) -> io::Result<u64>;
    fn create_key(&self, key: &RegPath) -> io::Result<()>;
    /// Create the key if needed and write the value.
    fn write_value(&self, key: &RegPath, value: &RawValue) -> io::Result<()>;
    /// Delete the key with everything beneath it.
    fn delete_key_tree(&self, key: &RegPath) -> io::Result<()>;
    fn delete_value(&self, key: &RegPath, name: &str) -> io::Result<()>;

    fn env_var(&self, name: &str) -> Option<String>;
    fn is_elevated(&self) -> bool;
    fn create_restore_point(&self, description: &str) -> io::Result<()>;
    fn launch(&self, launch: &Launch) -> io::Result<Box<dyn Running>>;
    /// Ask Windows to delete a file or empty folder at the next restart.
    fn delete_on_reboot(&self, path: &Path) -> io::Result<()>;
    /// Start an installer while recording the files and registry entries it
    /// and every process it starts create, change or delete.
    fn trace_install(&self, launch: &Launch) -> io::Result<Box<dyn Tracing>>;
    /// Every other account with a profile on this PC.
    fn other_accounts(&self) -> Vec<Account>;
    /// Unload every account hive loaded on demand. Called after each scan,
    /// removal and restore: a hive left loaded breaks that account's next
    /// sign-in.
    fn release_account_hives(&self) -> io::Result<()>;
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
