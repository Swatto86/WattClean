//! Registry addresses, raw values and the installed-program record.

use serde::{Deserialize, Serialize};

/// The registry roots the engine reads. `HKEY_CLASSES_ROOT` is never used
/// directly: it is a merged view of the two `Software\Classes` keys, which
/// are addressed explicitly so a deletion knows which one it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Hive {
    #[serde(rename = "HKLM")]
    LocalMachine,
    #[serde(rename = "HKCU")]
    CurrentUser,
    /// `HKEY_USERS`: other accounts' hives. The path's first component names
    /// the account (see [`RegPath::account_root`]).
    #[serde(rename = "HKU")]
    Users,
}

impl Hive {
    pub fn short(self) -> &'static str {
        match self {
            Self::LocalMachine => "HKLM",
            Self::CurrentUser => "HKCU",
            Self::Users => "HKU",
        }
    }
}

/// The key names another account's hives are addressed by under `HKU`.
pub const ACCOUNT_PREFIX: &str = "Account-";
pub const CLASSES_SUFFIX: &str = "_Classes";

/// A key: hive plus a backslash-separated path without leading or trailing
/// separators. Always the 64-bit view; 32-bit data is addressed through its
/// real `WOW6432Node` path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RegPath {
    pub hive: Hive,
    pub path: String,
}

impl RegPath {
    pub fn new(hive: Hive, path: &str) -> Self {
        Self {
            hive,
            path: path.trim_matches('\\').to_owned(),
        }
    }

    pub fn child(&self, name: &str) -> Self {
        if self.path.is_empty() {
            Self::new(self.hive, name)
        } else {
            Self::new(self.hive, &format!("{}\\{}", self.path, name))
        }
    }

    pub fn parent(&self) -> Option<Self> {
        self.path
            .rsplit_once('\\')
            .map(|(parent, _)| Self::new(self.hive, parent))
    }

    pub fn name(&self) -> &str {
        self.path.rsplit('\\').next().unwrap_or_default()
    }

    pub fn depth(&self) -> usize {
        if self.path.is_empty() {
            0
        } else {
            self.path.split('\\').count()
        }
    }

    pub fn display(&self) -> String {
        format!("{}\\{}", self.hive.short(), self.path)
    }

    /// Case-insensitive identity, as the registry compares names.
    pub fn folded(&self) -> String {
        format!("{}\\{}", self.hive.short(), self.path.to_lowercase())
    }

    /// The root of another account's registry (`HKU\Account-<name>`), which
    /// the machine loads from the account's `NTUSER.DAT` when it is not
    /// signed in; `account_classes_root` is its `UsrClass.dat`.
    pub fn account_root(account: &str) -> Self {
        Self::new(Hive::Users, &format!("{ACCOUNT_PREFIX}{account}"))
    }

    pub fn account_classes_root(account: &str) -> Self {
        Self::new(
            Hive::Users,
            &format!("{ACCOUNT_PREFIX}{account}{CLASSES_SUFFIX}"),
        )
    }

    /// For a key under `HKU`: the account it belongs to, whether it is in the
    /// account's classes hive, and the path inside that hive.
    pub fn account_part(&self) -> Option<(String, bool, String)> {
        if self.hive != Hive::Users {
            return None;
        }
        let (mount, rest) = self.path.split_once('\\').unwrap_or((&self.path, ""));
        let name = mount.strip_prefix(ACCOUNT_PREFIX)?;
        let (account, classes) = match name.strip_suffix(CLASSES_SUFFIX) {
            Some(account) => (account, true),
            None => (name, false),
        };
        Some((account.to_owned(), classes, rest.to_owned()))
    }

    /// True when `self` is `other` or lies beneath it.
    pub fn is_within(&self, other: &Self) -> bool {
        if self.hive != other.hive {
            return false;
        }
        let mine = self.path.to_lowercase();
        let theirs = other.path.to_lowercase();
        theirs.is_empty() || mine == theirs || mine.starts_with(&format!("{theirs}\\"))
    }
}

pub const REG_SZ: u32 = 1;
pub const REG_EXPAND_SZ: u32 = 2;
pub const REG_BINARY: u32 = 3;
pub const REG_DWORD: u32 = 4;
pub const REG_MULTI_SZ: u32 = 7;
pub const REG_QWORD: u32 = 11;

/// A value exactly as the registry stores it, so a backup restores it
/// byte for byte whatever its type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawValue {
    pub name: String,
    pub kind: u32,
    #[serde(with = "hex")]
    pub bytes: Vec<u8>,
}

impl RawValue {
    pub fn string(name: &str, text: &str) -> Self {
        Self::text(name, REG_SZ, text)
    }

    pub fn expand_string(name: &str, text: &str) -> Self {
        Self::text(name, REG_EXPAND_SZ, text)
    }

    fn text(name: &str, kind: u32, text: &str) -> Self {
        let bytes = text
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        Self {
            name: name.to_owned(),
            kind,
            bytes,
        }
    }

    pub fn dword(name: &str, value: u32) -> Self {
        Self {
            name: name.to_owned(),
            kind: REG_DWORD,
            bytes: value.to_le_bytes().to_vec(),
        }
    }

    /// The value as text: strings as stored, multi-strings joined by newlines.
    pub fn as_text(&self) -> Option<String> {
        if !matches!(self.kind, REG_SZ | REG_EXPAND_SZ | REG_MULTI_SZ) {
            return None;
        }
        let units: Vec<u16> = self
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let text = String::from_utf16_lossy(&units);
        let parts: Vec<&str> = text.split('\0').filter(|part| !part.is_empty()).collect();
        Some(parts.join("\n"))
    }

    pub fn as_u32(&self) -> Option<u32> {
        match (self.kind, self.bytes.as_slice()) {
            (REG_DWORD, [a, b, c, d]) => Some(u32::from_le_bytes([*a, *b, *c, *d])),
            _ => None,
        }
    }
}

/// Look a value up by name, as the registry does: case-insensitively.
pub fn find_value<'a>(values: &'a [RawValue], name: &str) -> Option<&'a RawValue> {
    values
        .iter()
        .find(|value| value.name.eq_ignore_ascii_case(name))
}

/// Where a program is registered, which decides who may remove it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// 64-bit machine-wide entry.
    Machine64,
    /// 32-bit machine-wide entry (`WOW6432Node`).
    Machine32,
    /// The current user's own entry; needs no administrator rights.
    User,
}

/// One entry of the Windows "installed programs" list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    /// The uninstall key's display path; stable while the entry exists.
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
    /// `YYYY-MM-DD`, from the entry or else the key's last change.
    pub install_date: Option<String>,
    pub size_kb: Option<u64>,
    pub install_location: Option<String>,
    pub uninstall_command: Option<String>,
    pub quiet_uninstall_command: Option<String>,
    pub modify_command: Option<String>,
    pub icon: Option<String>,
    pub website: Option<String>,
    pub comments: Option<String>,
    pub registry_key: RegPath,
    pub scope: Scope,
    pub msi_product_code: Option<String>,
    pub system_component: bool,
    /// A bundle installer's cached setup (`BundleCachePath`), under Package Cache.
    pub bundle_cache: Option<String>,
    /// The bundle's dependency provider key (`BundleProviderKey`).
    pub bundle_provider: Option<String>,
}

mod hex {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    use std::fmt::Write;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            // Writing to a String cannot fail.
            let _ = write!(text, "{byte:02x}");
        }
        serializer.serialize_str(&text)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.len() % 2 != 0 {
            return Err(D::Error::custom("hex text has an odd length"));
        }
        (0..text.len())
            .step_by(2)
            .map(|at| {
                text.get(at..at + 2)
                    .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                    .ok_or_else(|| D::Error::custom("hex text holds a non-hex character"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reg_path_relations() {
        let key = RegPath::new(Hive::LocalMachine, r"\SOFTWARE\Acme\Widget\");
        assert_eq!(key.path, r"SOFTWARE\Acme\Widget");
        assert_eq!(key.name(), "Widget");
        assert_eq!(key.depth(), 3);
        assert_eq!(key.parent().unwrap().path, r"SOFTWARE\Acme");
        let upper = RegPath::new(Hive::LocalMachine, r"software\ACME");
        assert!(key.is_within(&upper));
        assert!(!upper.is_within(&key));
        assert!(!RegPath::new(Hive::LocalMachine, r"SOFTWARE\AcmeX").is_within(&upper));
        assert!(!RegPath::new(Hive::CurrentUser, r"SOFTWARE\Acme").is_within(&upper));
    }

    #[test]
    fn raw_values_round_trip_through_json() {
        let values = vec![
            RawValue::string("Name", "Acme Widget"),
            RawValue::dword("Size", 1234),
            RawValue {
                name: "Blob".into(),
                kind: REG_BINARY,
                bytes: vec![0, 255, 16],
            },
        ];
        let json = serde_json::to_string(&values).unwrap();
        assert!(json.contains("\"00ff10\""), "{json}");
        let back: Vec<RawValue> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, values);
        assert_eq!(back[0].as_text().as_deref(), Some("Acme Widget"));
        assert_eq!(back[1].as_u32(), Some(1234));
        assert_eq!(back[2].as_text(), None);
    }

    #[test]
    fn multi_strings_join_with_newlines() {
        let mut bytes = Vec::new();
        for unit in "one\0two\0\0".encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        let value = RawValue {
            name: "List".into(),
            kind: REG_MULTI_SZ,
            bytes,
        };
        assert_eq!(value.as_text().as_deref(), Some("one\ntwo"));
    }

    #[test]
    fn malformed_hex_is_rejected() {
        let bad = r#"{"name":"x","kind":3,"bytes":"0g"}"#;
        assert!(serde_json::from_str::<RawValue>(bad).is_err());
    }
}
