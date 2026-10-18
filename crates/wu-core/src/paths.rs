//! Windows path comparison done the way the file system does it:
//! case-insensitively, either slash, whole components only.

use std::path::{Path, PathBuf};

use crate::System;

/// Lower-case, backslash-separated, no trailing separator. For comparison
/// only; never use the result to open a file.
pub fn fold(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let trimmed = text.trim_end_matches('\\');
    if trimmed.ends_with(':') {
        // Keep `c:\` a root rather than the drive-relative `c:`.
        format!("{trimmed}\\")
    } else {
        trimmed.to_owned()
    }
}

/// True when `path` is `base` or lies beneath it, by whole components.
pub fn is_within(path: &Path, base: &Path) -> bool {
    let path = fold(path);
    let base = fold(base);
    if path == base {
        return true;
    }
    let prefix = if base.ends_with('\\') {
        base
    } else {
        format!("{base}\\")
    };
    path.starts_with(&prefix)
}

pub fn same(a: &Path, b: &Path) -> bool {
    fold(a) == fold(b)
}

/// Replace `%NAME%` with the variable's value; unknown names stay as written,
/// as `ExpandEnvironmentStrings` leaves them.
pub fn expand(system: &dyn System, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match system.env_var(name).filter(|_| !name.is_empty()) {
                    Some(value) => out.push_str(&value),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The first absolute Windows path in `text`, ending at the first `.exe`,
/// `.dll`, `.sys`, `.ocx` or `.cpl` (or a closing quote), as command lines and
/// registry data embed them: `"C:\App\a.exe" -x`, `C:\App\a.dll,Entry`, `@C:\App\a.exe,-101`.
pub fn embedded_path(text: &str) -> Option<PathBuf> {
    // ASCII folding keeps byte offsets identical to `text`.
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let start = (0..bytes.len().saturating_sub(2)).find(|&at| {
        bytes[at].is_ascii_alphabetic()
            && bytes[at + 1] == b':'
            && bytes[at + 2] == b'\\'
            && (at == 0 || !bytes[at - 1].is_ascii_alphanumeric())
    })?;
    let tail = &lower[start..];
    let end = [".exe", ".dll", ".sys", ".ocx", ".cpl"]
        .iter()
        .filter_map(|ext| tail.find(ext).map(|at| at + ext.len()))
        .min()
        .or_else(|| tail.find(['"', ',', '|']))
        .unwrap_or(tail.len());
    let original = text.get(start..start + end)?;
    Some(PathBuf::from(original.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeSystem;

    #[test]
    fn containment_is_by_component() {
        let base = Path::new(r"C:\Program Files\Acme");
        assert!(is_within(Path::new(r"c:/program files/acme/bin"), base));
        assert!(is_within(Path::new(r"C:\Program Files\Acme\"), base));
        assert!(!is_within(Path::new(r"C:\Program Files\AcmeTools"), base));
        assert!(is_within(Path::new(r"D:\x"), Path::new(r"D:\")));
        assert!(!is_within(Path::new(r"C:\Program Files"), base));
    }

    #[test]
    fn expands_known_variables_only() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::empty(dir.path());
        system.set_env("ProgramFiles", r"C:\PF");
        assert_eq!(expand(&system, r"%ProgramFiles%\A"), r"C:\PF\A");
        assert_eq!(expand(&system, r"%Nope%\A %"), r"%Nope%\A %");
        assert_eq!(expand(&system, "100%%"), "100%%");
    }

    #[test]
    fn finds_paths_inside_command_lines_and_data() {
        let cases = [
            (r#""C:\App Dir\a.exe" -x"#, Some(r"C:\App Dir\a.exe")),
            (r"C:\App\a.dll,Entry", Some(r"C:\App\a.dll")),
            (r"@C:\App\a.exe,-101", Some(r"C:\App\a.exe")),
            (
                r"v2.30|Action=Allow|App=C:\App\b.exe|Name=x|",
                Some(r"C:\App\b.exe"),
            ),
            (r"C:\Data\folder", Some(r"C:\Data\folder")),
            ("no path here", None),
        ];
        for (text, want) in cases {
            assert_eq!(embedded_path(text), want.map(PathBuf::from), "{text}");
        }
    }
}
