//! Whose registry a scan reads: this account's, and on request every other
//! account's, each reached through its own hives.

use std::io;

use crate::model::{Hive, RegPath};
use crate::system::System;

/// One account's registry: its hive root, its `Software` key and its own
/// `Classes` key (`UsrClass.dat`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRoot {
    /// `None` for the account running the app.
    pub account: Option<String>,
    pub root: RegPath,
    pub software: RegPath,
    pub classes: RegPath,
}

impl UserRoot {
    pub fn current() -> Self {
        Self {
            account: None,
            root: RegPath::new(Hive::CurrentUser, ""),
            software: RegPath::new(Hive::CurrentUser, "Software"),
            classes: RegPath::new(Hive::CurrentUser, r"Software\Classes"),
        }
    }

    fn other(name: &str) -> Self {
        let root = RegPath::account_root(name);
        Self {
            account: Some(name.to_owned()),
            software: root.child("Software"),
            root,
            classes: RegPath::account_classes_root(name),
        }
    }
}

/// This account, plus every other account whose registry can be opened when
/// `all` is set. An account that cannot be read is left out with a note.
pub fn users(system: &dyn System, all: bool, notes: &mut Vec<String>) -> Vec<UserRoot> {
    let mut users = vec![UserRoot::current()];
    if !all {
        return users;
    }
    for account in system.other_accounts() {
        let user = UserRoot::other(&account.name);
        match system.subkeys(&user.software) {
            Ok(_) => users.push(user),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => notes.push(format!(
                "{}'s registry was not scanned: it needs administrator rights, or Windows has it in use.",
                account.name
            )),
            Err(error) => notes.push(format!(
                "{}'s registry was not scanned: {error}",
                account.name
            )),
        }
    }
    users
}
