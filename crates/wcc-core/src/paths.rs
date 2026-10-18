//! Comparing paths the way Windows does, and the guard every deletion passes:
//! protected folders (drive roots, Windows, the profile and its known
//! folders) and the keep list (BleachBit's whitelist) are never removed.

use std::path::{Component, Path, PathBuf};

/// A path as Windows compares it: case-insensitive, either separator,
/// `.` and `..` resolved, no trailing separator, no `\\?\` prefix.
pub fn key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| text.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or(text);
    let mut parts: Vec<String> = Vec::new();
    let mut prefix = String::new();
    for component in Path::new(&text).components() {
        match component {
            Component::Prefix(p) => prefix = p.as_os_str().to_string_lossy().to_lowercase(),
            Component::RootDir => prefix.push('\\'),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(name) => parts.push(name.to_string_lossy().to_lowercase()),
        }
    }
    let mut out = prefix;
    for (i, part) in parts.iter().enumerate() {
        if i > 0 || !(out.is_empty() || out.ends_with('\\')) {
            out.push('\\');
        }
        out.push_str(part);
    }
    out
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

/// Named folders below the root, ignoring the drive and root markers.
fn depth(key: &str) -> usize {
    Path::new(key)
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
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
