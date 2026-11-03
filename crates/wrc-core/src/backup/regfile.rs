//! A backup as a `.reg` file, so it can be read, kept or merged with
//! Registry Editor without this app.

use std::fmt::Write;

use super::capture::KeyTree;
use super::{Backup, Saved};
use crate::keyname::long_name;
use crate::model::{REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_QWORD, REG_SZ, RawValue, RegPath};

/// The file's text. Write it with [`reg_file_bytes`]: Registry Editor
/// expects UTF-16 with a byte-order mark.
pub fn to_reg_file(backup: &Backup) -> String {
    let mut text = String::from("Windows Registry Editor Version 5.00\r\n");
    let _ = write!(
        text,
        "\r\n; WattClean backup {} — {}\r\n",
        backup.id,
        comment(&backup.label)
    );
    for item in &backup.items {
        let Some(saved) = &item.saved else { continue };
        if item.key.account_part().is_some() {
            let _ = write!(
                text,
                "\r\n; Left out: {} is in another account's registry, which Registry Editor cannot import.\r\n",
                item.key.display()
            );
            continue;
        }
        match saved {
            Saved::Hive(file) => {
                let _ = write!(
                    text,
                    "\r\n; Left out: {} is a whole hive, kept as the file {file}.\r\n",
                    item.key.display()
                );
            }
            Saved::Key(tree) => write_key(&mut text, &item.key, tree),
            Saved::Value(value) => {
                let name = long_name(&item.key);
                // A `]` or a line break in a key name would close or break
                // the `[key]` line; such a key is left to the JSON backup.
                if exportable(&name) {
                    let _ = write!(text, "\r\n[{}]\r\n", name);
                    write_value(&mut text, value);
                }
            }
        }
    }
    text
}

/// UTF-16 little-endian with a byte-order mark.
pub fn reg_file_bytes(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

fn write_key(text: &mut String, key: &RegPath, tree: &KeyTree) {
    let name = long_name(key);
    if !exportable(&name) {
        return;
    }
    let _ = write!(text, "\r\n[{}]\r\n", name);
    for value in &tree.values {
        write_value(text, value);
    }
    for child in &tree.keys {
        write_key(text, &key.child(&child.name), &child.tree);
    }
}

/// A key name a `.reg` file can hold on one `[key]` line. Unlike a value
/// name it cannot be quoted or escaped there.
fn exportable(name: &str) -> bool {
    !name.contains(['\r', '\n', ']'])
}

fn write_value(text: &mut String, value: &RawValue) {
    let name = if value.name.is_empty() {
        "@".to_owned()
    } else {
        format!("\"{}\"", escape(&value.name))
    };
    let data = match (value.kind, value.bytes.as_slice()) {
        (REG_SZ, bytes) => match plain_string(bytes) {
            Some(plain) => format!("\"{}\"", escape(&plain)),
            None => format!("hex(1):{}", hex(bytes)),
        },
        (REG_DWORD, [a, b, c, d]) => format!("dword:{:08x}", u32::from_le_bytes([*a, *b, *c, *d])),
        (REG_EXPAND_SZ, bytes) => format!("hex(2):{}", hex(bytes)),
        (REG_MULTI_SZ, bytes) => format!("hex(7):{}", hex(bytes)),
        (REG_QWORD, bytes) => format!("hex(b):{}", hex(bytes)),
        (3, bytes) => format!("hex:{}", hex(bytes)),
        (kind, bytes) => format!("hex({kind:x}):{}", hex(bytes)),
    };
    let _ = write!(text, "{name}={data}\r\n");
}

/// A string that `"…"` notation can carry: one terminating NUL, no NUL or
/// line break inside.
fn plain_string(bytes: &[u8]) -> Option<String> {
    let (pairs, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    let (last, body) = units.split_last()?;
    if *last != 0 || body.contains(&0) {
        return None;
    }
    let text = String::from_utf16(body).ok()?;
    (!text.contains(['\r', '\n'])).then_some(text)
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

/// One line of comment text: a line break in the label must not become a
/// registry directive on a line of its own, so only the first line is kept.
fn comment(text: &str) -> String {
    text.split(['\r', '\n'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for (at, byte) in bytes.iter().enumerate() {
        if at > 0 {
            out.push(',');
        }
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use crate::backup::capture::Child;
    use crate::backup::{BackupKind, Item, ItemState};
    use crate::model::Hive;

    #[test]
    fn writes_what_registry_editor_reads() {
        let tree = KeyTree {
            values: vec![
                RawValue::string("", r#"C:\A "B""#),
                RawValue::dword("Count", 10),
                RawValue::expand_string("Path", "%X%"),
            ],
            keys: vec![Child {
                name: "Sub".into(),
                tree: KeyTree::default(),
            }],
            security: None,
        };
        let backup = Backup {
            version: 1,
            id: "1".into(),
            created: 1,
            kind: BackupKind::Manual,
            label: "test".into(),
            items: vec![Item {
                key: RegPath::new(Hive::CurrentUser, r"Software\Acme"),
                value: None,
                category: None,
                detail: String::new(),
                saved: Some(Saved::Key(tree)),
                security: None,
                state: ItemState::Kept,
            }],
        };
        let text = to_reg_file(&backup);
        assert!(text.starts_with("Windows Registry Editor Version 5.00\r\n"));
        assert!(
            text.contains("[HKEY_CURRENT_USER\\Software\\Acme]\r\n@=\"C:\\\\A \\\"B\\\"\"\r\n"),
            "{text}"
        );
        assert!(text.contains("\"Count\"=dword:0000000a\r\n"));
        assert!(text.contains("\"Path\"=hex(2):25,00,58,00,25,00,00,00\r\n"));
        assert!(text.contains("[HKEY_CURRENT_USER\\Software\\Acme\\Sub]\r\n"));
        let bytes = reg_file_bytes("W");
        assert_eq!(bytes, vec![0xff, 0xfe, b'W', 0]);
    }

    #[test]
    fn a_line_break_in_the_label_cannot_become_a_directive() {
        let mut backup = Backup {
            version: 1,
            id: "1".into(),
            created: 1,
            kind: BackupKind::Manual,
            label: "x\r\n[-HKEY_CURRENT_USER\\Software\\Keep]".into(),
            items: Vec::new(),
        };
        let text = to_reg_file(&backup);
        assert!(!text.contains("[-HKEY_CURRENT_USER"), "{text}");
        backup.label = "plain".into();
        assert_eq!(escape("a\r\nb"), "a\\r\\nb");
    }

    #[test]
    fn a_key_name_that_would_close_the_line_is_not_exported() {
        let tree = KeyTree {
            values: Vec::new(),
            keys: vec![Child {
                name: "bad]\r\n[-HKEY_CURRENT_USER\\Software\\Keep".into(),
                tree: KeyTree::default(),
            }],
            security: None,
        };
        let backup = Backup {
            version: 1,
            id: "1".into(),
            created: 1,
            kind: BackupKind::Manual,
            label: "plain".into(),
            items: vec![Item {
                key: RegPath::new(Hive::CurrentUser, r"Software\Acme"),
                value: None,
                category: None,
                detail: String::new(),
                saved: Some(Saved::Key(tree)),
                security: None,
                state: ItemState::Kept,
            }],
        };
        let text = to_reg_file(&backup);
        assert!(
            text.contains(r"[HKEY_CURRENT_USER\Software\Acme]"),
            "{text}"
        );
        assert!(!text.contains("[-HKEY_CURRENT_USER"), "{text}");
    }

    #[test]
    fn odd_strings_fall_back_to_hex() {
        assert_eq!(plain_string(&[b'a', 0, 0, 0]), Some("a".into()));
        assert_eq!(plain_string(&[b'a', 0]), None, "no terminator");
        assert_eq!(
            plain_string(&[b'a', 0, 0, 0, b'b', 0, 0, 0]),
            None,
            "inner NUL"
        );
    }
}
