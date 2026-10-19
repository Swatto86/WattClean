//! Key paths as people type them and as Windows tools show them.

use crate::model::{Hive, RegPath};

/// The longest path the registry allows (255 characters per key name and
/// 16,383 for a whole path); anything longer is not a key.
const MAX_PATH: usize = 16_383;

/// A key typed by a person or pasted from Registry Editor:
/// `HKEY_CURRENT_USER\Software\X`, `HKCU\Software\X`, or with Registry
/// Editor's `Computer\` in front. `HKEY_CLASSES_ROOT` is refused because it
/// is a merged view of two keys; the person names the one they mean.
pub fn parse_key(text: &str) -> Result<RegPath, String> {
    let trimmed = text.trim().trim_matches('\\');
    let without_computer = strip_prefix_ignore_case(trimmed, "Computer\\").unwrap_or(trimmed);
    if without_computer.is_empty() {
        return Err("Enter a registry key, such as HKEY_CURRENT_USER\\Software\\Example.".into());
    }
    if without_computer.len() > MAX_PATH {
        return Err("That key path is longer than the registry allows.".into());
    }
    if without_computer.chars().any(char::is_control) {
        return Err("A key path cannot contain control characters.".into());
    }
    let (root, rest) = without_computer
        .split_once('\\')
        .unwrap_or((without_computer, ""));
    let hive = match root.to_ascii_uppercase().as_str() {
        "HKEY_LOCAL_MACHINE" | "HKLM" => Hive::LocalMachine,
        "HKEY_CURRENT_USER" | "HKCU" => Hive::CurrentUser,
        "HKEY_USERS" | "HKU" => Hive::Users,
        "HKEY_CLASSES_ROOT" | "HKCR" => {
            return Err("HKEY_CLASSES_ROOT combines two keys. Use HKEY_LOCAL_MACHINE\\SOFTWARE\\Classes or HKEY_CURRENT_USER\\Software\\Classes.".into());
        }
        _ => {
            return Err(format!(
                "\"{root}\" is not a registry root. Start with HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER or HKEY_USERS."
            ));
        }
    };
    if rest.split('\\').any(str::is_empty) && !rest.is_empty() {
        return Err("A key path cannot contain an empty name (\\\\).".into());
    }
    Ok(RegPath::new(hive, rest))
}

/// `HKEY_LOCAL_MACHINE\SOFTWARE\X`, as Registry Editor and `.reg` files name it.
pub fn long_name(key: &RegPath) -> String {
    let root = match key.hive {
        Hive::LocalMachine => "HKEY_LOCAL_MACHINE",
        Hive::CurrentUser => "HKEY_CURRENT_USER",
        Hive::Users => "HKEY_USERS",
    };
    if key.path.is_empty() {
        root.to_owned()
    } else {
        format!("{root}\\{}", key.path)
    }
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| text.get(prefix.len()..))
        .flatten()
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_ways_people_write_keys() {
        let cases = [
            (
                r"HKEY_CURRENT_USER\Software\Acme",
                Hive::CurrentUser,
                r"Software\Acme",
            ),
            (r"hkcu\Software\Acme\", Hive::CurrentUser, r"Software\Acme"),
            (
                r"Computer\HKEY_LOCAL_MACHINE\SOFTWARE",
                Hive::LocalMachine,
                "SOFTWARE",
            ),
            ("HKLM", Hive::LocalMachine, ""),
            (
                r"HKU\S-1-5-21-1\Software",
                Hive::Users,
                r"S-1-5-21-1\Software",
            ),
        ];
        for (text, hive, path) in cases {
            let key = parse_key(text).unwrap();
            assert_eq!((key.hive, key.path.as_str()), (hive, path), "{text}");
        }
    }

    #[test]
    fn refuses_what_is_not_one_key() {
        for bad in [
            "",
            "   ",
            r"HKCR\.txt",
            r"HKEY_CLASSES_ROOT\CLSID",
            r"Software\Acme",
            r"HKCU\Software\\Acme",
            "HKCU\\Soft\u{0}ware",
        ] {
            assert!(parse_key(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn long_names_round_trip() {
        let key = RegPath::new(Hive::LocalMachine, r"SOFTWARE\Acme");
        assert_eq!(long_name(&key), r"HKEY_LOCAL_MACHINE\SOFTWARE\Acme");
        assert_eq!(parse_key(&long_name(&key)).unwrap(), key);
        assert_eq!(long_name(&RegPath::new(Hive::Users, "")), "HKEY_USERS");
    }
}
