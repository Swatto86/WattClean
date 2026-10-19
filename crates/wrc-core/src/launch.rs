//! Whether this executable may be recorded for Windows to start later (at
//! sign-in, or by a scheduled task). Recording a development build or a copy
//! in Downloads or Temp would leave Windows starting a file that soon moves
//! or vanishes, so only an installed copy is accepted.

use std::path::{Component, Path};

use crate::paths::is_within;

/// Why `exe` must not be recorded, or `None` when it is an installed copy.
pub fn not_installed(exe: &Path, temp: &Path, downloads: Option<&Path>) -> Option<String> {
    if downloads.is_some_and(|downloads| is_within(exe, downloads)) {
        return Some(format!(
            "this copy runs from your Downloads folder ({}). Install WattClean first.",
            exe.display()
        ));
    }
    if is_within(exe, temp) {
        return Some(format!(
            "this copy runs from a temporary folder ({}). Install WattClean first.",
            exe.display()
        ));
    }
    if is_cargo_build(exe) {
        return Some(format!(
            "this is a development build ({}). Only the installed app can do this.",
            exe.display()
        ));
    }
    None
}

/// A binary under a cargo `target\debug` or `target\release` tree.
fn is_cargo_build(path: &Path) -> bool {
    let parts: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();
    parts
        .windows(2)
        .any(|pair| pair[0] == "target" && matches!(pair[1].as_str(), "debug" | "release"))
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn only_an_installed_copy_is_accepted() {
        let temp = Path::new(r"C:\Users\A\AppData\Local\Temp");
        let downloads = Some(Path::new(r"C:\Users\A\Downloads"));
        let installed = Path::new(r"C:\Program Files\WattClean\wattclean.exe");
        assert_eq!(not_installed(installed, temp, downloads), None);
        for bad in [
            r"C:\Users\A\Downloads\wattclean.exe",
            r"C:\Users\A\AppData\Local\Temp\x\wattclean.exe",
            r"C:\src\WattClean\target\debug\wattclean.exe",
            r"C:\src\WattClean\target\release\wattclean.exe",
        ] {
            assert!(
                not_installed(Path::new(bad), temp, downloads).is_some(),
                "{bad}"
            );
        }
    }
}
