//! The registry, through winreg. Keys are written `HKCU\Software\…`.

use std::io;

use wcc_core::system::{Registry, split_hive};
use winreg::RegKey;
use winreg::enums::{
    HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, KEY_SET_VALUE,
};

pub struct WindowsRegistry;

fn open(key: &str) -> Option<(RegKey, String)> {
    let (hive, rest) = split_hive(key)?;
    let root = RegKey::predef(match hive {
        "HKCU" => HKEY_CURRENT_USER,
        "HKLM" => HKEY_LOCAL_MACHINE,
        "HKCR" => HKEY_CLASSES_ROOT,
        _ => HKEY_USERS,
    });
    Some((root, rest.to_string()))
}

fn bad_key(key: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{key} is not a registry key"),
    )
}

impl Registry for WindowsRegistry {
    fn key_exists(&self, key: &str) -> bool {
        open(key).is_some_and(|(root, rest)| {
            rest.is_empty() || root.open_subkey_with_flags(&rest, KEY_READ).is_ok()
        })
    }

    fn value_exists(&self, key: &str, name: &str) -> bool {
        open(key).is_some_and(|(root, rest)| {
            root.open_subkey_with_flags(&rest, KEY_READ)
                .is_ok_and(|k| k.get_raw_value(name).is_ok())
        })
    }

    fn subkeys(&self, key: &str) -> Vec<String> {
        let Some((root, rest)) = open(key) else {
            return Vec::new();
        };
        let key = if rest.is_empty() {
            Ok(root)
        } else {
            root.open_subkey_with_flags(&rest, KEY_READ)
        };
        key.map(|key| key.enum_keys().flatten().collect())
            .unwrap_or_default()
    }

    fn delete_key(&self, key: &str) -> io::Result<()> {
        let (root, rest) = open(key).ok_or_else(|| bad_key(key))?;
        if rest.is_empty() {
            return Err(bad_key(key));
        }
        root.delete_subkey_all(&rest)
    }

    fn delete_value(&self, key: &str, name: &str) -> io::Result<()> {
        let (root, rest) = open(key).ok_or_else(|| bad_key(key))?;
        root.open_subkey_with_flags(&rest, KEY_SET_VALUE)?
            .delete_value(name)
    }
}

/// Creates a key with one value, for this crate's own tests.
#[cfg(test)]
pub fn create_for_test(key: &str, value: &str) -> io::Result<()> {
    let (root, rest) = open(key).ok_or_else(|| bad_key(key))?;
    let (created, _) = root.create_subkey(&rest)?;
    created.set_value(value, &"test")
}
