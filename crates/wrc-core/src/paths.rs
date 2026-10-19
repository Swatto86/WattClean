//! Windows paths as registry data writes them: inside command lines, with
//! variables, compared the way the file system compares them.

use std::path::Path;

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

/// File types a command line or registry data names directly. A path ends
/// at the first of these that is followed by a separator.
const EXTENSIONS: [&str; 26] = [
    ".exe", ".dll", ".ocx", ".cpl", ".sys", ".com", ".scr", ".msc", ".bat", ".cmd", ".vbs", ".js",
    ".ps1", ".tlb", ".olb", ".ax", ".drv", ".ico", ".wav", ".ttf", ".otf", ".ttc", ".fon", ".msi",
    ".chm", ".hta",
];

/// The file a command line or a piece of registry data names:
/// `"C:\App\a.exe" -x`, `C:\Program Files\App\a.exe /x`, `C:\App\a.dll,-101`,
/// `@C:\App\a.dll,-5`, and for `rundll32 C:\App\b.dll,Entry` the DLL it
/// loads. Returned as written (variables unexpanded); `None` for empty text,
/// or when where the program ends cannot be told (an unquoted command with a
/// space and no known file type), so that nothing is judged from a guess.
pub fn command_target(command: &str) -> Option<String> {
    let text = command.trim().trim_start_matches('@').trim();
    if text.is_empty() {
        return None;
    }
    let (program, rest) = split_program(text)?;
    let file = program
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&program)
        .to_ascii_lowercase();
    if file == "rundll32" || file == "rundll32.exe" {
        let library = rest.trim();
        if library.is_empty() {
            return Some(program);
        }
        return split_program(library).map(|(library, _)| library);
    }
    Some(program)
}

/// The program at the start of `text` and what follows it.
fn split_program(text: &str) -> Option<(String, &str)> {
    if let Some(quoted) = text.strip_prefix('"') {
        return Some(match quoted.split_once('"') {
            Some((program, rest)) => (program.trim().to_owned(), rest),
            None => (quoted.trim().to_owned(), ""),
        });
    }
    // A bare first word (`rundll32 …`, `notepad %1`) is the program itself.
    if let Some((first, rest)) = text.split_once(' ')
        && !first.contains(['\\', '/', '"'])
    {
        return Some((first.to_owned(), rest));
    }
    if let Some(end) = extension_end(text) {
        return Some((text[..end].trim().to_owned(), &text[end..]));
    }
    // No known file type ends it: a path up to the first comma, unless a
    // space leaves unclear where the program stops and its arguments start.
    let end = text.find(',').unwrap_or(text.len());
    let program = text[..end].trim();
    (!program.contains(char::is_whitespace)).then(|| (program.to_owned(), &text[end..]))
}

/// Where the first known extension ends that is followed by the end of the
/// text, a space (unless a folder's name goes on), a comma, a quote, a slash
/// (`a.exe/S`) or a tab.
fn extension_end(text: &str) -> Option<usize> {
    // ASCII folding keeps byte offsets identical to `text`.
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut best: Option<usize> = None;
    for ext in EXTENSIONS {
        let mut from = 0;
        while let Some(at) = lower[from..].find(ext) {
            let end = from + at + ext.len();
            let boundary = match bytes.get(end) {
                None | Some(b',' | b'"' | b'\t' | b'/') => true,
                Some(b' ') => !folder_goes_on(&text[end..]),
                Some(_) => false,
            };
            if boundary {
                best = Some(best.map_or(end, |known| known.min(end)));
                break;
            }
            from = end;
        }
    }
    best
}

/// After `C:\Program Files\Booking.com` comes ` Helper\helper.exe`: the
/// folder's name went on rather than an argument starting. Arguments are
/// words up to one that starts like a switch, a quote, a variable or a path
/// of its own (`-x`, `/x`, `"…"`, `%1`, `C:\…`, `\\server`); a word with a
/// backslash before any of those continues the path.
fn folder_goes_on(rest: &str) -> bool {
    for word in rest.split_whitespace() {
        let bytes = word.as_bytes();
        let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
        if drive || matches!(bytes[0], b'-' | b'/' | b'"' | b'%' | b'\\') {
            return false;
        }
        if word.contains('\\') {
            return true;
        }
    }
    false
}

/// An absolute local path (`C:\…`), as opposed to a bare file name that
/// Windows would look for on the search path, or a network share.
pub fn is_absolute_local(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
}

#[cfg(test)]
#[cfg(windows)]
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
    }

    #[test]
    fn expands_known_variables_only() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::empty(dir.path());
        system.set_env("ProgramFiles", r"C:\PF");
        assert_eq!(expand(&system, r"%ProgramFiles%\A"), r"C:\PF\A");
        assert_eq!(expand(&system, r"%Nope%\A %"), r"%Nope%\A %");
    }

    #[test]
    fn finds_the_file_a_command_names() {
        let cases = [
            (r#""C:\App Dir\a.exe" -x"#, r"C:\App Dir\a.exe"),
            (
                r"C:\Program Files\App\a.exe /S",
                r"C:\Program Files\App\a.exe",
            ),
            (r"C:\App\a.dll,-101", r"C:\App\a.dll"),
            (r"@C:\App\a.dll,-5", r"C:\App\a.dll"),
            (r"C:\my.exe.d\b.exe", r"C:\my.exe.d\b.exe"),
            (
                r"%SystemRoot%\System32\rundll32.exe C:\App\b.dll,Entry",
                r"C:\App\b.dll",
            ),
            (r#"rundll32 "C:\A B\c.dll",Run"#, r"C:\A B\c.dll"),
            (r"notepad.exe %1", "notepad.exe"),
            (r"C:\Data\folder", r"C:\Data\folder"),
            (r"C:\Media\ding.wav", r"C:\Media\ding.wav"),
        ];
        for (text, want) in cases {
            assert_eq!(command_target(text).as_deref(), Some(want), "{text}");
        }
        assert_eq!(command_target("  "), None);
    }

    /// Where a program ends is sometimes unknowable; then nothing is judged,
    /// so a live program is never called missing.
    #[test]
    fn programs_are_never_cut_inside_a_folder_or_run_on_into_arguments() {
        let cases = [
            (
                r"C:\Program Files\Booking.com Helper\helper.exe --run",
                Some(r"C:\Program Files\Booking.com Helper\helper.exe"),
            ),
            (
                r"C:\Tools\Node.js Tools\node.exe x.js",
                Some(r"C:\Tools\Node.js Tools\node.exe"),
            ),
            (r"C:\App\a.exe/S", Some(r"C:\App\a.exe")),
            (r"C:\App\a.exe C:\Data\b.txt", Some(r"C:\App\a.exe")),
            (
                r#"rundll32.exe "C:\Program Files\Acme, Inc\a.dll",Run"#,
                Some(r"C:\Program Files\Acme, Inc\a.dll"),
            ),
            (r"C:\Program Files\Foo\foo.jar -x", None),
            (r"C:\Program Files\Foo\foosvc -run", None),
            (r"C:\Data\folder", Some(r"C:\Data\folder")),
        ];
        for (text, want) in cases {
            assert_eq!(command_target(text).as_deref(), want, "{text}");
        }
    }

    #[test]
    fn only_drive_paths_are_absolute_local() {
        assert!(is_absolute_local(r"C:\x"));
        assert!(!is_absolute_local(r"\\server\share\x.exe"));
        assert!(!is_absolute_local("notepad.exe"));
        assert!(!is_absolute_local("C:x"));
    }
}
