//! Where a `.lnk` shortcut points, read from its LinkInfo block
//! ([MS-SHLLINK] §2.3) without asking the shell, so a broken shortcut whose
//! target is already gone is still recognised.

use std::path::{Path, PathBuf};

const HEADER_SIZE: usize = 0x4C;
const HAS_ID_LIST: u32 = 0x1;
const HAS_LINK_INFO: u32 = 0x2;
const VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 0x1;

/// The local target path of a shortcut file, or `None` when it is not a
/// shell link or has no local path (a network or shell-namespace target).
pub fn target_of(file: &Path) -> Option<PathBuf> {
    let extension = file.extension()?.to_string_lossy().to_lowercase();
    if extension != "lnk" {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    parse(&bytes)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<usize> {
    let pair = bytes.get(at..at + 2)?;
    Some(usize::from(u16::from_le_bytes([pair[0], pair[1]])))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let quad = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
}

fn offset_at(bytes: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32_at(bytes, at)?).ok()
}

pub fn parse(bytes: &[u8]) -> Option<PathBuf> {
    if offset_at(bytes, 0)? != HEADER_SIZE {
        return None;
    }
    let flags = u32_at(bytes, 0x14)?;
    if flags & HAS_LINK_INFO == 0 {
        return None;
    }
    let mut at = HEADER_SIZE;
    if flags & HAS_ID_LIST != 0 {
        at += 2 + u16_at(bytes, at)?;
    }
    let info = bytes.get(at..at + offset_at(bytes, at)?)?;
    let header_size = offset_at(info, 4)?;
    if u32_at(info, 8)? & VOLUME_ID_AND_LOCAL_BASE_PATH == 0 {
        return None;
    }
    let (base, suffix) = if header_size >= 0x24 {
        (
            wide_string(info, offset_at(info, 0x1C)?)?,
            wide_string(info, offset_at(info, 0x20)?).unwrap_or_default(),
        )
    } else {
        (
            narrow_string(info, offset_at(info, 0x10)?)?,
            narrow_string(info, offset_at(info, 0x18)?).unwrap_or_default(),
        )
    };
    (!base.is_empty()).then(|| PathBuf::from(format!("{base}{suffix}")))
}

fn narrow_string(bytes: &[u8], at: usize) -> Option<String> {
    let tail = bytes.get(at..)?;
    let end = tail.iter().position(|&byte| byte == 0)?;
    Some(tail[..end].iter().map(|&byte| char::from(byte)).collect())
}

fn wide_string(bytes: &[u8], at: usize) -> Option<String> {
    let tail = bytes.get(at..)?;
    let units: Vec<u16> = tail
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|&unit| unit != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// The address inside a `.url` internet shortcut (an INI file with `URL=`).
pub fn url_of(file: &Path) -> Option<String> {
    let extension = file.extension()?.to_string_lossy().to_lowercase();
    if extension != "url" {
        return None;
    }
    let text = std::fs::read_to_string(file).ok()?;
    text.lines()
        .map(str::trim)
        .find_map(|line| {
            line.strip_prefix("URL=")
                .or_else(|| line.strip_prefix("url="))
        })
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty())
}

/// A minimal shell link with an ANSI LinkInfo block pointing at `target`,
/// for the fake machine and the tests.
#[cfg(any(test, feature = "fake"))]
pub fn link_bytes(target: &str) -> Vec<u8> {
    let mut bytes = vec![0u8; HEADER_SIZE];
    bytes[0] = 0x4C;
    bytes[0x14] = 0x02;
    let volume = [0x11u8, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0x10, 0, 0, 0, 0];
    let base_offset = 0x1C + volume.len();
    let suffix_offset = base_offset + target.len() + 1;
    let size = suffix_offset + 1;
    let mut info = Vec::new();
    for field in [size, 0x1C, 1, 0x1C, base_offset, 0, suffix_offset] {
        info.extend(u32::try_from(field).unwrap_or(u32::MAX).to_le_bytes());
    }
    info.extend(volume);
    info.extend(target.as_bytes());
    info.extend([0, 0]);
    bytes.extend(info);
    bytes
}

#[cfg(test)]
pub(crate) mod tests {
    pub use super::link_bytes as link_to;
    use super::*;

    #[test]
    fn reads_the_local_base_path() {
        let bytes = link_to(r"C:\Program Files\Acme\Widget\widget.exe");
        assert_eq!(
            parse(&bytes),
            Some(PathBuf::from(r"C:\Program Files\Acme\Widget\widget.exe"))
        );
    }

    #[test]
    fn skips_an_id_list_before_the_link_info() {
        let plain = link_to(r"C:\A\b.exe");
        let mut bytes = plain[..HEADER_SIZE].to_vec();
        bytes[0x14] = 0x03;
        bytes.extend([4, 0, 0xAA, 0xBB, 0xCC, 0xDD]);
        bytes.extend(&plain[HEADER_SIZE..]);
        assert_eq!(parse(&bytes), Some(PathBuf::from(r"C:\A\b.exe")));
    }

    #[test]
    fn rejects_other_files_and_truncation() {
        assert_eq!(parse(b"not a link"), None);
        let bytes = link_to(r"C:\A\b.exe");
        assert_eq!(parse(&bytes[..HEADER_SIZE + 10]), None);
        let mut no_info = bytes.clone();
        no_info[0x14] = 0;
        assert_eq!(parse(&no_info), None);
    }
}
