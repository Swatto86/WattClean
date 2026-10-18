//! How names are compared: folding, generic words, product and publisher
//! names stripped of versions, architectures and legal suffixes.

/// Names too common to identify one program; a folder or key with one of
/// these names is never matched on name alone.
const GENERIC: &[&str] = &[
    "app",
    "apps",
    "application",
    "agent",
    "client",
    "crashpad",
    "crashreporter",
    "elevate",
    "helper",
    "host",
    "launcher",
    "main",
    "server",
    "service",
    "uninst",
    "unins000",
    "bin",
    "cache",
    "common",
    "commonfiles",
    "config",
    "data",
    "default",
    "desktop",
    "documents",
    "download",
    "downloads",
    "drivers",
    "help",
    "install",
    "installer",
    "lib",
    "local",
    "logs",
    "microsoft",
    "packages",
    "plugins",
    "program",
    "programfiles",
    "programs",
    "resources",
    "settings",
    "setup",
    "shared",
    "software",
    "system",
    "temp",
    "tmp",
    "tools",
    "uninstall",
    "update",
    "updater",
    "user",
    "users",
    "windows",
    "x64",
    "x86",
];

/// Fold a name for comparison: lower case, letters and digits only, plus
/// `+` and `#` so "Notepad++" stays distinct from Windows' "Notepad".
pub fn name_key(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_alphanumeric() || *ch == '+' || *ch == '#')
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn is_generic(key: &str) -> bool {
    key.len() < 3 || GENERIC.contains(&key)
}

/// The product name without version, architecture or edition decorations:
/// "7-Zip 24.08 (x64)" → "7-Zip", "Mozilla Firefox (x64 en-GB)" → "Mozilla Firefox".
pub fn product_name(display_name: &str) -> String {
    let mut text = String::new();
    let mut depth = 0u32;
    for ch in display_name.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => text.push(ch),
            _ => {}
        }
    }
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|word| !is_decoration(word))
        .collect();
    let joined = words.join(" ");
    joined
        .trim_matches(|ch: char| ch == '-' || ch == ',' || ch.is_whitespace())
        .to_owned()
}

fn is_decoration(word: &str) -> bool {
    let lower = word
        .trim_matches(|ch: char| ch == ',' || ch == '-')
        .to_lowercase();
    let version = lower.strip_prefix('v').unwrap_or(&lower);
    let looks_like_version = version.contains('.')
        && version.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        && version
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-');
    looks_like_version
        || [
            "x64", "x86", "x86_64", "amd64", "arm64", "64-bit", "32-bit", "64bit", "32bit",
            "win64", "win32",
        ]
        .contains(&lower.as_str())
}

/// The publisher's distinctive part: "Acme Software Ltd." → "acmesoftware".
pub fn publisher_key(publisher: &str) -> Option<String> {
    const SUFFIXES: &[&str] = &[
        "inc",
        "inc.",
        "ltd",
        "ltd.",
        "llc",
        "corp",
        "corp.",
        "corporation",
        "co.",
        "co",
        "gmbh",
        "ag",
        "s.a.",
        "sa",
        "s.r.o.",
        "bv",
        "b.v.",
        "limited",
        "plc",
        "pty",
        "oy",
        "ab",
    ];
    let words: Vec<&str> = publisher
        .split([' ', ','])
        .filter(|word| !word.is_empty() && !SUFFIXES.contains(&word.to_lowercase().as_str()))
        .collect();
    let key = name_key(&words.join(" "));
    (!is_generic(&key)).then_some(key)
}
