//! Comparing paths, and the guard every deletion passes: protected folders
//! (drive roots, the system's folders, the profile and its known folders)
//! and the keep list (BleachBit's whitelist) are never removed.
//!
//! Comparison is one text rule on every platform: either separator,
//! case-insensitive. On Linux, where two names may differ only by case,
//! that can only make the guard refuse more, never less.

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::Kind;

/// A path as the guard compares it: case-insensitive, either separator,
/// `.` and `..` resolved, no trailing separator, no `\\?\` prefix. Keys
/// always use `\` (the Unix path `/home/a` is `\home\a`). On macOS a
/// folder's other spellings fold into its short one (`fold_mac_aliases`).
pub fn key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| text.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or(text);
    let (prefix, rooted, rest) = split_prefix(&text);
    let mut out = prefix.to_lowercase();
    if rooted {
        out.push('\\');
    }
    let mut parts = normal_parts(rest);
    if cfg!(target_os = "macos") && prefix.is_empty() && rooted {
        parts = fold_mac_aliases(parts, boot_volume_names());
    }
    for (i, part) in parts.iter().enumerate() {
        if i > 0 || !(out.is_empty() || out.ends_with('\\')) {
            out.push('\\');
        }
        out.push_str(&part.to_lowercase());
    }
    out
}

/// The drive (`C:`) or share (`\\server\share`) a backslashed path starts
/// with, whether a root separator follows it, and the rest.
fn split_prefix(text: &str) -> (&str, bool, &str) {
    let bytes = text.as_bytes();
    let (prefix, rest) = if let Some(after) = text.strip_prefix(r"\\") {
        let server = after.find('\\').unwrap_or(after.len());
        let share_end = after
            .get(server + 1..)
            .and_then(|share| share.find('\\'))
            .map_or(after.len(), |end| server + 1 + end);
        text.split_at(2 + share_end)
    } else if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        text.split_at(2)
    } else {
        ("", text)
    };
    match rest.strip_prefix('\\') {
        Some(rest) => (prefix, true, rest),
        None => (prefix, false, rest),
    }
}

/// macOS reaches one folder by several paths: the data volume's firmlinks
/// (`/System/Volumes/Data/Users/a` is `/Users/a`), `/private` (`/private/var`
/// is `/var`) and the start-up disk's link in `/Volumes` (`/Volumes/Macintosh
/// HD/Users` is `/Users`). The short spelling stands for all of them, so the
/// guard sees one folder however a path names it.
fn fold_mac_aliases<'a>(mut parts: Vec<&'a str>, boot_names: &[String]) -> Vec<&'a str> {
    let is = |part: Option<&&str>, name: &str| part.is_some_and(|p| p.eq_ignore_ascii_case(name));
    if is(parts.first(), "volumes")
        && parts
            .get(1)
            .is_some_and(|name| boot_names.iter().any(|b| b.eq_ignore_ascii_case(name)))
    {
        parts.drain(..2);
    }
    if is(parts.first(), "system") && is(parts.get(1), "volumes") && is(parts.get(2), "data") {
        parts.drain(..3);
    }
    if is(parts.first(), "private")
        && (is(parts.get(1), "var") || is(parts.get(1), "tmp") || is(parts.get(1), "etc"))
    {
        parts.remove(0);
    }
    parts
}

/// The names in `/Volumes` that are links to `/`: the start-up disk.
#[cfg(target_os = "macos")]
fn boot_volume_names() -> &'static [String] {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        fs::read_dir("/Volumes")
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| fs::read_link(e.path()).is_ok_and(|t| t == Path::new("/")))
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    })
}

#[cfg(not(target_os = "macos"))]
fn boot_volume_names() -> &'static [String] {
    &[]
}

/// The named parts of a path after its prefix, `.` and `..` resolved.
fn normal_parts(rest: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    for part in rest.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    parts
}

/// A definition's path in this platform's own spelling. Definitions may use
/// either separator (the built-in templates are shared by every platform);
/// on Linux and macOS a backslash in a definition is always a separator.
pub fn native(text: &str) -> String {
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text.replace('\\', "/")
    }
}

/// `path` is `root` or inside it, comparing whole components.
pub fn is_within(path: &str, root: &str) -> bool {
    if root.is_empty() {
        return false;
    }
    path == root
        || (path.len() > root.len()
            && path.starts_with(root)
            && (root.ends_with('\\') || path.as_bytes().get(root.len()) == Some(&b'\\')))
}

/// Whether an entry is a folder to delete: a real folder, or a link to one
/// (a junction or directory symlink, which only `remove_dir` can remove).
pub fn kind_of(meta: &fs::Metadata) -> Kind {
    if meta.is_dir() || is_dir_link(meta) {
        Kind::Dir
    } else {
        Kind::File
    }
}

#[cfg(windows)]
fn is_dir_link(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    meta.file_type().is_symlink() && meta.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
}

#[cfg(not(windows))]
fn is_dir_link(_: &fs::Metadata) -> bool {
    false
}

/// A socket, pipe or device: a running program's door or the system's,
/// never a file to clean (sessions keep their sockets in the temporary
/// folder on Linux and macOS).
#[cfg(unix)]
pub fn is_special(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    let kind = meta.file_type();
    kind.is_socket() || kind.is_fifo() || kind.is_block_device() || kind.is_char_device()
}

#[cfg(not(unix))]
pub fn is_special(_: &fs::Metadata) -> bool {
    false
}

/// `base` joined with a relative path written with either separator, as
/// the test machine and the tests write Windows-shaped paths on every
/// platform.
#[cfg(any(test, feature = "fake"))]
pub fn join(base: &Path, relative: &str) -> PathBuf {
    base.join(native(relative))
}

/// Named folders below the root, ignoring the drive and root markers.
fn depth(key: &str) -> usize {
    let (_, _, rest) = split_prefix(key);
    normal_parts(rest).len()
}

/// Why a path may not be removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A drive root, a top-level folder, or a folder Windows or the profile
    /// depends on (or a folder holding one of them).
    Protected,
    /// On the keep list, inside a kept folder, or a folder holding one.
    Kept,
}

#[derive(Debug, Clone, Default)]
pub struct Guard {
    protected: Vec<String>,
    keep: Vec<String>,
}

impl Guard {
    pub fn new(protected: &[PathBuf], keep: &[PathBuf]) -> Self {
        let keys = |list: &[PathBuf]| {
            let mut keys: Vec<String> = list
                .iter()
                .map(|p| key(p))
                .filter(|k| !k.is_empty())
                .collect();
            keys.sort();
            keys.dedup();
            keys
        };
        Self {
            protected: keys(protected),
            keep: keys(keep),
        }
    }

    /// Whether `path` may be deleted (a file, or a folder once empty).
    pub fn check(&self, path: &Path) -> Result<(), Refusal> {
        let k = key(path);
        if depth(&k) < 2 || self.protected.iter().any(|p| is_within(p, &k)) {
            return Err(Refusal::Protected);
        }
        if self
            .keep
            .iter()
            .any(|kept| is_within(&k, kept) || is_within(kept, &k))
        {
            return Err(Refusal::Kept);
        }
        Ok(())
    }

    /// Whether anything at or below `path` is kept, so a walk can skip it.
    pub fn is_kept(&self, path: &Path) -> bool {
        let k = key(path);
        self.keep.iter().any(|kept| is_within(&k, kept))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> PathBuf {
        PathBuf::from(text)
    }

    #[test]
    fn keys_ignore_case_separators_and_dots() {
        assert_eq!(
            key(&p(r"C:/Users/Tester/./AppData/x/../Temp/")),
            r"c:\users\tester\appdata\temp"
        );
        assert_eq!(key(&p(r"\\?\C:\Temp")), r"c:\temp");
        assert_eq!(key(&p(r"C:\")), r"c:\");
    }

    #[test]
    fn macos_spellings_of_one_folder_fold_into_one() {
        let boot = vec!["Macintosh HD".to_string()];
        let fold = |text: &str| {
            fold_mac_aliases(text.split('/').filter(|p| !p.is_empty()).collect(), &boot).join("/")
        };
        assert_eq!(
            fold("/System/Volumes/Data/Users/x/Documents"),
            "Users/x/Documents"
        );
        assert_eq!(fold("/System/Volumes/Data"), "");
        assert_eq!(fold("/private/var/folders/ab/T"), "var/folders/ab/T");
        assert_eq!(fold("/System/Volumes/Data/private/tmp/x"), "tmp/x");
        assert_eq!(fold("/Volumes/Macintosh HD/Users/x"), "Users/x");
        assert_eq!(fold("/Volumes/USB/Users/x"), "Volumes/USB/Users/x");
        assert_eq!(fold("/private/Users/x"), "private/Users/x");
        assert_eq!(
            fold("/Users/x/System/Volumes/Data"),
            "Users/x/System/Volumes/Data"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_guard_protects_a_folder_by_any_of_its_macos_spellings() {
        let guard = Guard::new(&[p("/Users/t"), p("/var/folders/ab/T")], &[]);
        assert_eq!(
            guard.check(&p("/System/Volumes/Data/Users/t")),
            Err(Refusal::Protected)
        );
        assert_eq!(
            guard.check(&p("/private/var/folders/ab/T")),
            Err(Refusal::Protected)
        );
        assert_eq!(
            guard.check(&p("/System/Volumes/Data")),
            Err(Refusal::Protected)
        );
        assert_eq!(guard.check(&p("/private/var/folders/ab/T/x.tmp")), Ok(()));
    }

    #[test]
    fn containment_is_by_whole_component() {
        assert!(is_within(r"c:\temp\a", r"c:\temp"));
        assert!(is_within(r"c:\temp", r"c:\temp"));
        assert!(!is_within(r"c:\temporary", r"c:\temp"));
        assert!(is_within(r"c:\anything", r"c:\"));
    }

    #[test]
    fn the_guard_refuses_roots_known_folders_and_their_parents() {
        let guard = Guard::new(&[p(r"C:\Users\T\AppData\Local\Temp")], &[]);
        assert_eq!(guard.check(&p(r"C:\")), Err(Refusal::Protected));
        assert_eq!(guard.check(&p(r"C:\Windows")), Err(Refusal::Protected));
        assert_eq!(
            guard.check(&p(r"C:\Users\T\AppData\Local\Temp")),
            Err(Refusal::Protected)
        );
        assert_eq!(
            guard.check(&p(r"C:\Users\T\AppData")),
            Err(Refusal::Protected)
        );
        assert_eq!(
            guard.check(&p(r"C:\Users\T\AppData\Local\Temp\x.tmp")),
            Ok(())
        );
    }

    #[test]
    fn the_keep_list_covers_its_contents_and_its_parents() {
        let guard = Guard::new(&[], &[p(r"C:\Users\T\AppData\Local\App\Keep")]);
        assert_eq!(
            guard.check(&p(r"c:\users\t\appdata\local\app\keep\a.txt")),
            Err(Refusal::Kept)
        );
        assert_eq!(
            guard.check(&p(r"C:\Users\T\AppData\Local\App")),
            Err(Refusal::Kept)
        );
        assert_eq!(
            guard.check(&p(r"C:\Users\T\AppData\Local\App\Other")),
            Ok(())
        );
        assert!(guard.is_kept(&p(r"C:\Users\T\AppData\Local\App\Keep\deep")));
        assert!(!guard.is_kept(&p(r"C:\Users\T\AppData\Local\App")));
    }
}
