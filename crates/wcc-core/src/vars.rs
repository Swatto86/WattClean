//! `%VARIABLE%` expansion in cleaner paths.
//!
//! A variable may stand for several folders (`%ProgramFiles%` is both
//! Program Files folders, as winapp2.ini expects), so one pattern can expand
//! to several paths. A variable the machine does not know drops the pattern:
//! a literal `%NAME%` folder is never cleaned by mistake.

/// Looks a variable up by name (case-insensitive); empty when unknown.
pub trait Vars {
    fn values(&self, name: &str) -> Vec<String>;
}

impl<F: Fn(&str) -> Vec<String>> Vars for F {
    fn values(&self, name: &str) -> Vec<String> {
        self(name)
    }
}

/// Every expansion of `pattern`. A `%` with no closing `%` stays literal.
pub fn expand(pattern: &str, vars: &dyn Vars) -> Vec<String> {
    let mut results = vec![String::new()];
    let mut rest = pattern;
    while let Some(start) = rest.find('%') {
        let after = &rest[start + 1..];
        let Some(len) = after.find('%') else {
            break;
        };
        let name = &after[..len];
        let literal = &rest[..start];
        if name.is_empty() || !is_name(name) {
            // `%%` or `50% off%`: not a variable, keep the first `%` as text.
            for r in &mut results {
                r.push_str(literal);
                r.push('%');
            }
            rest = after;
            continue;
        }
        let values = vars.values(name);
        if values.is_empty() {
            return Vec::new();
        }
        let mut next = Vec::with_capacity(results.len() * values.len());
        for r in &results {
            for v in &values {
                let mut s = r.clone();
                s.push_str(literal);
                s.push_str(v.trim_end_matches(['\\', '/']));
                next.push(s);
            }
        }
        results = next;
        rest = &after[len + 1..];
    }
    for r in &mut results {
        r.push_str(rest);
        // `C:` alone means the current folder on C:, so name the root.
        if r.len() == 2 && r.ends_with(':') {
            r.push('\\');
        }
    }
    results.sort();
    results.dedup();
    results
}

/// Every expansion of an exclusion, or `None` when it cannot be expanded:
/// it would silently protect nothing, so the search must be refused.
/// OneDrive is the exception: it is absent on many PCs, and then there is
/// nothing to protect.
pub fn expand_exclusion(pattern: &str, vars: &dyn Vars) -> Option<Vec<String>> {
    let expansions = expand(pattern, vars);
    let unknown = expansions.is_empty()
        && unresolved(pattern, vars)
            .iter()
            .any(|name| name != "ONEDRIVE");
    (!unknown).then_some(expansions)
}

/// The variables in `pattern` that have no value on this machine.
fn unresolved(pattern: &str, vars: &dyn Vars) -> Vec<String> {
    let mut missing = Vec::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('%') {
        let after = &rest[start + 1..];
        let Some(len) = after.find('%') else {
            break;
        };
        let name = &after[..len];
        if !name.is_empty() && is_name(name) {
            if vars.values(name).is_empty() {
                missing.push(name.to_ascii_uppercase());
            }
            rest = &after[len + 1..];
        } else {
            rest = after;
        }
    }
    missing
}

fn is_name(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '(' | ')' | '-'))
}

/// Replaces `{name}` placeholders from a definition's own `vars` table.
/// A value may itself use placeholders (`profile = '{root}\*'`), so this
/// repeats a few times until nothing changes.
pub fn fill_placeholders(text: &str, vars: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for _ in 0..4 {
        let before = out.clone();
        for (name, value) in vars {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        if out == before {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(name: &str) -> Vec<String> {
        match name.to_ascii_uppercase().as_str() {
            "APPDATA" => vec![r"C:\Users\T\AppData\Roaming\".into()],
            "PROGRAMFILES" => vec![r"C:\Program Files".into(), r"C:\Program Files (x86)".into()],
            "TEMP" => vec![r"C:\Users\T\AppData\Local\Temp".into()],
            _ => Vec::new(),
        }
    }

    #[test]
    fn expands_case_insensitively_and_trims_trailing_separators() {
        assert_eq!(
            expand(r"%AppData%\Foo\*.log", &machine),
            vec![r"C:\Users\T\AppData\Roaming\Foo\*.log".to_string()]
        );
    }

    #[test]
    fn a_variable_with_several_values_multiplies_the_paths() {
        assert_eq!(
            expand(r"%ProgramFiles%\App\%temp%", &machine),
            vec![
                r"C:\Program Files (x86)\App\C:\Users\T\AppData\Local\Temp".to_string(),
                r"C:\Program Files\App\C:\Users\T\AppData\Local\Temp".to_string(),
            ]
        );
    }

    #[test]
    fn an_unknown_variable_drops_the_pattern() {
        assert!(expand(r"%NoSuchThing%\x", &machine).is_empty());
    }

    #[test]
    fn a_bare_drive_becomes_its_root() {
        let drive = |name: &str| -> Vec<String> {
            if name.eq_ignore_ascii_case("SystemDrive") {
                vec!["C:".into()]
            } else {
                Vec::new()
            }
        };
        assert_eq!(expand("%SystemDrive%", &drive), vec![r"C:\".to_string()]);
        assert_eq!(
            expand(r"%SystemDrive%\Temp", &drive),
            vec![r"C:\Temp".to_string()]
        );
    }

    #[test]
    fn stray_percent_signs_stay_literal() {
        assert_eq!(expand(r"C:\100%", &machine), vec![r"C:\100%".to_string()]);
        assert_eq!(
            expand(r"C:\a b% c%\%TEMP%", &machine),
            vec![r"C:\a b% c%\C:\Users\T\AppData\Local\Temp".to_string()]
        );
    }

    #[test]
    fn placeholders_come_from_the_definition() {
        let vars = vec![
            ("cache".to_string(), r"{profile}".to_string()),
            ("profile".to_string(), r"{root}\*".to_string()),
            ("root".to_string(), r"%LOCALAPPDATA%\X".to_string()),
        ];
        assert_eq!(
            fill_placeholders(r"{cache}\Cache", &vars),
            r"%LOCALAPPDATA%\X\*\Cache"
        );
    }
}
