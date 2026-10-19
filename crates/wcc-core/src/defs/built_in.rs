//! Which definition files are built in, per platform: the shared templates
//! and each platform's own cleaners (`unix/` serves Linux and macOS both).

use crate::system::Platform;

/// Templates every platform's cleaners use (Chromium, Firefox, Electron).
const TEMPLATES: (&str, &str) = ("templates.toml", include_str!("templates.toml"));

/// The built-in definitions for Windows, by file name.
pub const WINDOWS: &[(&str, &str)] = &[
    TEMPLATES,
    ("windows/system.toml", include_str!("windows/system.toml")),
    (
        "windows/browsers.toml",
        include_str!("windows/browsers.toml"),
    ),
    (
        "windows/applications.toml",
        include_str!("windows/applications.toml"),
    ),
    (
        "windows/developer.toml",
        include_str!("windows/developer.toml"),
    ),
    ("windows/games.toml", include_str!("windows/games.toml")),
    (
        "windows/deepscan.toml",
        include_str!("windows/deepscan.toml"),
    ),
];

/// The built-in definitions for Linux.
pub const LINUX: &[(&str, &str)] = &[
    TEMPLATES,
    ("linux/system.toml", include_str!("linux/system.toml")),
    ("linux/packages.toml", include_str!("linux/packages.toml")),
    ("linux/browsers.toml", include_str!("linux/browsers.toml")),
    (
        "linux/applications.toml",
        include_str!("linux/applications.toml"),
    ),
    ("unix/developer.toml", include_str!("unix/developer.toml")),
    ("linux/games.toml", include_str!("linux/games.toml")),
    ("unix/deepscan.toml", include_str!("unix/deepscan.toml")),
];

/// The built-in definitions for macOS.
pub const MACOS: &[(&str, &str)] = &[
    TEMPLATES,
    ("macos/system.toml", include_str!("macos/system.toml")),
    ("macos/packages.toml", include_str!("macos/packages.toml")),
    ("macos/browsers.toml", include_str!("macos/browsers.toml")),
    (
        "macos/applications.toml",
        include_str!("macos/applications.toml"),
    ),
    ("unix/developer.toml", include_str!("unix/developer.toml")),
    ("macos/developer.toml", include_str!("macos/developer.toml")),
    ("macos/games.toml", include_str!("macos/games.toml")),
    ("unix/deepscan.toml", include_str!("unix/deepscan.toml")),
];

/// The built-in definition files for a platform.
pub fn built_in_files(platform: Platform) -> &'static [(&'static str, &'static str)] {
    match platform {
        Platform::Windows => WINDOWS,
        Platform::Linux => LINUX,
        Platform::Macos => MACOS,
    }
}
