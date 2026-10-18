//! `*` and `?` wildcards, matched case-insensitively as Windows does, and
//! the paths a pattern of them names.

use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wildcard {
    pattern: Vec<char>,
}

impl Wildcard {
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.chars().flat_map(char::to_lowercase).collect(),
        }
    }

    pub fn has_wildcards(text: &str) -> bool {
        text.contains(['*', '?'])
    }

    /// `*.*` matches every name, as it does in Windows (even names with no dot).
    pub fn matches(&self, text: &str) -> bool {
        if self.pattern == ['*', '.', '*'] || self.pattern == ['*'] {
            return true;
        }
        let text: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
        let (mut p, mut t) = (0usize, 0usize);
        let mut star: Option<(usize, usize)> = None;
        while t < text.len() {
            match self.pattern.get(p) {
                Some('*') => {
                    star = Some((p, t));
                    p += 1;
                }
                Some('?') => {
                    p += 1;
                    t += 1;
                }
                Some(c) if Some(c) == text.get(t) => {
                    p += 1;
                    t += 1;
                }
                _ => match star {
                    Some((sp, st)) => {
                        p = sp + 1;
                        t = st + 1;
                        star = Some((sp, st + 1));
                    }
                    None => return false,
                },
            }
        }
        self.pattern[p..].iter().all(|c| *c == '*')
    }
}

/// Several wildcards separated by `;`, as winapp2.ini writes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WildcardSet(Vec<Wildcard>);

impl WildcardSet {
    pub fn new(list: &str) -> Self {
        Self(
            list.split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(Wildcard::new)
                .collect(),
        )
    }

    pub fn matches(&self, text: &str) -> bool {
        self.0.iter().any(|w| w.matches(text))
    }
}

/// Paths matching a pattern with `*` and `?` in any component. Folders
/// reached through a link are not searched.
pub fn glob(pattern: &str) -> Vec<PathBuf> {
    let normalized = pattern.replace('/', "\\");
    let path = Path::new(&normalized);
    let mut base = PathBuf::new();
    let mut rest: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir if rest.is_empty() => {
                base.push(component.as_os_str())
            }
            Component::Normal(name) => {
                let name = name.to_string_lossy().to_string();
                if rest.is_empty() && !Wildcard::has_wildcards(&name) {
                    base.push(&name);
                } else {
                    rest.push(name);
                }
            }
            Component::CurDir => {}
            _ => rest.push(component.as_os_str().to_string_lossy().to_string()),
        }
    }
    if rest.is_empty() {
        return if fs::symlink_metadata(&base).is_ok() {
            vec![base]
        } else {
            Vec::new()
        };
    }
    let mut current = vec![base];
    for (index, part) in rest.iter().enumerate() {
        let last = index + 1 == rest.len();
        let mut next = Vec::new();
        for dir in &current {
            if Wildcard::has_wildcards(part) {
                let wildcard = Wildcard::new(part);
                let Ok(entries) = fs::read_dir(dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !wildcard.matches(&name) {
                        continue;
                    }
                    let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                        continue;
                    };
                    if last || (meta.is_dir() && !meta.file_type().is_symlink()) {
                        next.push(entry.path());
                    }
                }
            } else {
                let candidate = dir.join(part);
                if fs::symlink_metadata(&candidate).is_ok() {
                    next.push(candidate);
                }
            }
        }
        next.sort();
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_like_windows() {
        let cases = [
            ("*.log", "Error.LOG", true),
            ("*.log", "error.log.old", false),
            ("*.*", "noextension", true),
            ("f?o*", "FOObar", true),
            ("f?o*", "fo", false),
            ("*cache*", "GPUCache", true),
            ("a*b*c", "aXXbYYc", true),
            ("a*b*c", "aXXbYY", false),
            ("", "", true),
            ("", "x", false),
        ];
        for (pattern, text, expected) in cases {
            assert_eq!(
                Wildcard::new(pattern).matches(text),
                expected,
                "{pattern} vs {text}"
            );
        }
    }

    #[test]
    fn sets_split_on_semicolons() {
        let set = WildcardSet::new("*.tmp; *.bak ;");
        assert!(set.matches("a.TMP"));
        assert!(set.matches("b.bak"));
        assert!(!set.matches("c.txt"));
    }
}
