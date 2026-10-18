//! Registry access through `winreg`, always in the 64-bit view.

use std::borrow::Cow;
use std::io;

use winreg::enums::{
    KEY_ALL_ACCESS, KEY_READ, KEY_WOW64_64KEY, REG_BINARY, REG_DWORD, REG_DWORD_BIG_ENDIAN,
    REG_EXPAND_SZ, REG_FULL_RESOURCE_DESCRIPTOR, REG_LINK, REG_MULTI_SZ, REG_NONE, REG_QWORD,
    REG_RESOURCE_LIST, REG_RESOURCE_REQUIREMENTS_LIST, REG_SZ, RegType,
};
use winreg::{RegKey, RegValue};
use wu_core::model::{RawValue, RegPath};

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

pub fn modified(root: &RegKey, key: &RegPath) -> io::Result<u64> {
    let time = open(root, key, KEY_READ)?
        .query_info()?
        .get_last_write_time_system();
    Ok(unix_seconds(
        i64::from(time.wYear),
        i64::from(time.wMonth),
        i64::from(time.wDay),
        u64::from(time.wHour) * 3600 + u64::from(time.wMinute) * 60 + u64::from(time.wSecond),
    ))
}

pub fn create(root: &RegKey, key: &RegPath) -> io::Result<RegKey> {
    root.create_subkey_with_flags(&key.path, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
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
    open(root, &parent, KEY_ALL_ACCESS)?.delete_subkey_all(key.name())
}

pub fn delete_value(root: &RegKey, key: &RegPath, name: &str) -> io::Result<()> {
    open(root, key, KEY_ALL_ACCESS)?.delete_value(name)
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

/// Seconds since the Unix epoch for a UTC civil date and time of day.
fn unix_seconds(year: i64, month: i64, day: i64, seconds_of_day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * shifted + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days).unwrap_or(0) * 86_400 + seconds_of_day
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_convert_to_unix_seconds() {
        assert_eq!(unix_seconds(1970, 1, 1, 0), 0);
        assert_eq!(unix_seconds(2000, 2, 29, 0), 951_782_400);
        assert_eq!(unix_seconds(2026, 10, 14, 61), 1_791_936_061);
        assert_eq!(unix_seconds(1960, 1, 1, 0), 0, "before 1970 clamps to zero");
    }

    #[test]
    fn every_supported_type_maps_both_ways() {
        for number in 0..=11 {
            assert_eq!(kind_number(&kind_type(number).unwrap()), number);
        }
        assert!(kind_type(12).is_err());
    }
}
