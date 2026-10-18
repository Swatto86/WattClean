//! Removing one setting from a JSON or INI file while keeping the rest, for
//! applications that keep history inside their settings file (BleachBit's
//! `json` and `ini` commands). Files are rewritten through a temporary file,
//! so a failure leaves the original as it was.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("wcc-tmp");
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _removed = fs::remove_file(&temp);
    })
}

fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// Walks `a/b/c` to the object holding `c`.
fn parent_mut<'a>(
    value: &'a mut Value,
    address: &str,
) -> Option<(&'a mut serde_json::Map<String, Value>, String)> {
    let mut parts: Vec<&str> = address.split('/').filter(|p| !p.is_empty()).collect();
    let last = parts.pop()?.to_string();
    let mut current = value;
    for part in parts {
        current = current.as_object_mut()?.get_mut(part)?;
    }
    Some((current.as_object_mut()?, last))
}

pub fn json_has(path: &Path, address: &str) -> bool {
    let Some(mut value) = read_json(path) else {
        return false;
    };
    parent_mut(&mut value, address).is_some_and(|(map, key)| map.contains_key(&key))
}

/// Removes the key; returns whether it was there.
pub fn json_remove(path: &Path, address: &str) -> io::Result<bool> {
    let text = fs::read_to_string(path)?;
    let mut value: Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(io::Error::other)?;
    let removed =
        parent_mut(&mut value, address).is_some_and(|(map, key)| map.remove(&key).is_some());
    if removed {
        write_atomically(path, &serde_json::to_vec(&value).map_err(io::Error::other)?)?;
    }
    Ok(removed)
}

/// INI files may be UTF-8 (with or without a byte-order mark) or UTF-16.
enum Encoding {
    Utf8 { bom: bool },
    Utf16,
}

fn read_ini(path: &Path) -> io::Result<(String, Encoding)> {
    let bytes = fs::read(path)?;
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return Ok((String::from_utf16_lossy(&units), Encoding::Utf16));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok((
            String::from_utf8_lossy(rest).into_owned(),
            Encoding::Utf8 { bom: true },
        ));
    }
    Ok((
        String::from_utf8_lossy(&bytes).into_owned(),
        Encoding::Utf8 { bom: false },
    ))
}

fn encode(text: &str, encoding: &Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf16 => {
            let mut out = vec![0xFF, 0xFE];
            out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            out
        }
        Encoding::Utf8 { bom } => {
            let mut out = if *bom {
                vec![0xEF, 0xBB, 0xBF]
            } else {
                Vec::new()
            };
            out.extend_from_slice(text.as_bytes());
            out
        }
    }
}

fn section_name(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix('[')?
        .strip_suffix(']')
        .map(str::trim)
}

fn parameter_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.starts_with([';', '#']) {
        return None;
    }
    trimmed.split_once('=').map(|(k, _)| k.trim())
}

/// The lines left after removing the section, or the parameter in it.
fn without(text: &str, section: &str, parameter: Option<&str>) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    let mut removed = false;
    for line in text.split_inclusive('\n') {
        if let Some(name) = section_name(line) {
            inside = name.eq_ignore_ascii_case(section);
            if inside && parameter.is_none() {
                removed = true;
                continue;
            }
        } else if inside {
            match parameter {
                None => continue,
                Some(p) if parameter_name(line).is_some_and(|k| k.eq_ignore_ascii_case(p)) => {
                    removed = true;
                    continue;
                }
                Some(_) => {}
            }
        }
        out.push_str(line);
    }
    (out, removed)
}

pub fn ini_has(path: &Path, section: &str, parameter: Option<&str>) -> bool {
    read_ini(path).is_ok_and(|(text, _)| without(&text, section, parameter).1)
}

/// Removes the section or parameter; returns whether anything went.
pub fn ini_remove(path: &Path, section: &str, parameter: Option<&str>) -> io::Result<bool> {
    let (text, encoding) = read_ini(path)?;
    let (kept, removed) = without(&text, section, parameter);
    if removed {
        write_atomically(path, &encode(&kept, &encoding))?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_keys_are_removed_by_address() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Preferences");
        fs::write(
            &path,
            r#"{"dns_prefetching":{"host_names":["a","b"],"keep":1},"other":true}"#,
        )
        .unwrap();
        assert!(json_has(&path, "dns_prefetching/host_names"));
        assert!(json_remove(&path, "dns_prefetching/host_names").unwrap());
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"dns_prefetching": {"keep": 1}, "other": true})
        );
        assert!(!json_has(&path, "dns_prefetching/host_names"));
        assert!(!json_remove(&path, "dns_prefetching/host_names").unwrap());
        assert!(!json_has(&path, "missing/deeper/key"));
    }

    #[test]
    fn ini_sections_and_parameters_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vlc-qt-interface.ini");
        let text = "[General]\r\nfoo=1\r\n[RecentsMRU]\r\nlist=a, b\r\ntimes=1, 2\r\n[Other]\r\nlist=keep\r\n";
        fs::write(&path, text).unwrap();
        assert!(ini_has(&path, "recentsmru", Some("LIST")));
        assert!(ini_remove(&path, "RecentsMRU", Some("list")).unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[General]\r\nfoo=1\r\n[RecentsMRU]\r\ntimes=1, 2\r\n[Other]\r\nlist=keep\r\n"
        );
        assert!(ini_remove(&path, "RecentsMRU", None).unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[General]\r\nfoo=1\r\n[Other]\r\nlist=keep\r\n"
        );
        assert!(!ini_has(&path, "RecentsMRU", None));
    }

    #[test]
    fn utf16_ini_files_stay_utf16() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.ini");
        fs::write(
            &path,
            encode("[Recent]\nFile1=x\n[Keep]\nA=1\n", &Encoding::Utf16),
        )
        .unwrap();
        assert!(ini_remove(&path, "Recent", None).unwrap());
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
        assert_eq!(read_ini(&path).unwrap().0, "[Keep]\nA=1\n");
    }
}
