//! How a registry value reads to a person: its type's name and its data.

use std::fmt::Write;

use crate::model::{
    REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_QWORD, REG_SZ, RawValue,
};

/// Data longer than this is cut, with an ellipsis, for lists and logs.
const SHOWN: usize = 300;

/// Registry Editor's name for a value type.
pub fn kind_name(kind: u32) -> &'static str {
    match kind {
        0 => "REG_NONE",
        REG_SZ => "REG_SZ",
        REG_EXPAND_SZ => "REG_EXPAND_SZ",
        REG_BINARY => "REG_BINARY",
        REG_DWORD => "REG_DWORD",
        5 => "REG_DWORD_BIG_ENDIAN",
        6 => "REG_LINK",
        REG_MULTI_SZ => "REG_MULTI_SZ",
        8 => "REG_RESOURCE_LIST",
        9 => "REG_FULL_RESOURCE_DESCRIPTOR",
        10 => "REG_RESOURCE_REQUIREMENTS_LIST",
        REG_QWORD => "REG_QWORD",
        _ => "unknown type",
    }
}

/// The value's data as Registry Editor shows it: text as written (lists
/// joined with `; `), numbers in hex and decimal, bytes in hex.
pub fn describe(value: &RawValue) -> String {
    let text = match value.kind {
        REG_SZ | REG_EXPAND_SZ => value.as_text().unwrap_or_default(),
        REG_MULTI_SZ => value
            .as_text()
            .unwrap_or_default()
            .split('\n')
            .collect::<Vec<_>>()
            .join("; "),
        REG_DWORD => match value.as_u32() {
            Some(number) => format!("0x{number:08x} ({number})"),
            None => hex(&value.bytes),
        },
        REG_QWORD => match <[u8; 8]>::try_from(value.bytes.as_slice()) {
            Ok(bytes) => {
                let number = u64::from_le_bytes(bytes);
                format!("0x{number:016x} ({number})")
            }
            Err(_) => hex(&value.bytes),
        },
        _ => hex(&value.bytes),
    };
    shorten(&text)
}

/// The value's name, or Registry Editor's `(Default)` for the unnamed one.
pub fn value_name(name: &str) -> &str {
    if name.is_empty() { "(Default)" } else { name }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 3);
    for (at, byte) in bytes.iter().enumerate() {
        if at > 0 {
            text.push(' ');
        }
        // Writing to a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn shorten(text: &str) -> String {
    match text.char_indices().nth(SHOWN) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_each_kind_as_regedit_does() {
        assert_eq!(describe(&RawValue::string("a", r"C:\x")), r"C:\x");
        assert_eq!(describe(&RawValue::dword("a", 255)), "0x000000ff (255)");
        let qword = RawValue {
            name: "q".into(),
            kind: REG_QWORD,
            bytes: 1u64.to_le_bytes().to_vec(),
        };
        assert_eq!(describe(&qword), "0x0000000000000001 (1)");
        let blob = RawValue {
            name: "b".into(),
            kind: REG_BINARY,
            bytes: vec![0, 171],
        };
        assert_eq!(describe(&blob), "00 ab");
        assert_eq!(kind_name(REG_MULTI_SZ), "REG_MULTI_SZ");
        assert_eq!(value_name(""), "(Default)");
    }

    #[test]
    fn long_data_is_cut_on_a_character_boundary() {
        let long = "é".repeat(SHOWN + 5);
        let shown = describe(&RawValue::string("a", &long));
        assert_eq!(shown.chars().count(), SHOWN + 1);
        assert!(shown.ends_with('…'));
    }
}
