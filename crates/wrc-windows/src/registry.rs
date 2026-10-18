//! Registry access through `winreg`, always in the 64-bit view. Each call
//! asks only for the rights it needs: administrators often lack full access
//! (`WRITE_DAC`, `WRITE_OWNER`) to keys such as `HKLM\SOFTWARE` itself, so
//! asking for `KEY_ALL_ACCESS` would refuse work they are allowed to do.

use std::borrow::Cow;
use std::io;

use winreg::enums::{
    KEY_READ, KEY_SET_VALUE, KEY_WOW64_64KEY, KEY_WRITE, REG_BINARY, REG_DWORD,
    REG_DWORD_BIG_ENDIAN, REG_EXPAND_SZ, REG_FULL_RESOURCE_DESCRIPTOR, REG_LINK, REG_MULTI_SZ,
    REG_NONE, REG_QWORD, REG_RESOURCE_LIST, REG_RESOURCE_REQUIREMENTS_LIST, REG_SZ, RegType,
};
use winreg::{RegKey, RegValue};
use wrc_core::model::{RawValue, RegPath};

fn open(root: &RegKey, key: &RegPath, access: u32) -> io::Result<RegKey> {
    root.open_subkey_with_flags(&key.path, access | KEY_WOW64_64KEY)
}

pub fn subkeys(root: &RegKey, key: &RegPath) -> io::Result<Vec<String>> {
    open(root, key, KEY_READ)?.enum_keys().collect()
}

/// Every value of the key. A value of a type the registry library cannot
/// represent fails the whole read, so a backup never silently drops it.
pub fn values(root: &RegKey, key: &RegPath) -> io::Result<Vec<RawValue>> {
    open(root, key, KEY_READ)?
        .enum_values()
        .map(|entry| {
            entry.map(|(name, value)| RawValue {
                name,
                kind: kind_number(&value.vtype),
                bytes: value.bytes.into_owned(),
            })
        })
        .collect()
}

pub fn create(root: &RegKey, key: &RegPath) -> io::Result<RegKey> {
    // Creating needs KEY_CREATE_SUB_KEY on the parent, which Windows checks
    // itself; the handle needs only to read and set values.
    root.create_subkey_with_flags(&key.path, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
        .map(|(created, _)| created)
}

pub fn write_value(root: &RegKey, key: &RegPath, value: &RawValue) -> io::Result<()> {
    let target = create(root, key)?;
    let raw = RegValue {
        bytes: Cow::Borrowed(&value.bytes),
        vtype: kind_type(value.kind)?,
    };
    target.set_raw_value(&value.name, &raw)
}

pub fn delete_tree(root: &RegKey, key: &RegPath) -> io::Result<()> {
    let parent = key.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a hive's top-level key is never deleted",
        )
    })?;
    // Deleting needs DELETE on the key and its subkeys, which RegDeleteTree
    // asks for itself; the parent handle only has to exist.
    open(root, &parent, KEY_READ)?.delete_subkey_all(key.name())
}

pub fn delete_value(root: &RegKey, key: &RegPath, name: &str) -> io::Result<()> {
    open(root, key, KEY_SET_VALUE)?.delete_value(name)
}

fn kind_number(kind: &RegType) -> u32 {
    match kind {
        REG_NONE => 0,
        REG_SZ => 1,
        REG_EXPAND_SZ => 2,
        REG_BINARY => 3,
        REG_DWORD => 4,
        REG_DWORD_BIG_ENDIAN => 5,
        REG_LINK => 6,
        REG_MULTI_SZ => 7,
        REG_RESOURCE_LIST => 8,
        REG_FULL_RESOURCE_DESCRIPTOR => 9,
        REG_RESOURCE_REQUIREMENTS_LIST => 10,
        REG_QWORD => 11,
    }
}

fn kind_type(kind: u32) -> io::Result<RegType> {
    Ok(match kind {
        0 => REG_NONE,
        1 => REG_SZ,
        2 => REG_EXPAND_SZ,
        3 => REG_BINARY,
        4 => REG_DWORD,
        5 => REG_DWORD_BIG_ENDIAN,
        6 => REG_LINK,
        7 => REG_MULTI_SZ,
        8 => REG_RESOURCE_LIST,
        9 => REG_FULL_RESOURCE_DESCRIPTOR,
        10 => REG_RESOURCE_REQUIREMENTS_LIST,
        11 => REG_QWORD,
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("registry value type {other} is not supported"),
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_type_maps_both_ways() {
        for number in 0..=11 {
            assert_eq!(kind_number(&kind_type(number).unwrap()), number);
        }
        assert!(kind_type(12).is_err());
    }
}
