//! This machine's package managers and application bundles.

use std::io;
use std::path::{Path, PathBuf};

use wu_core::packages::{BundleInfo, Output, PackageSystem};

pub struct UnixPackages {
    home: Option<PathBuf>,
}

impl UnixPackages {
    pub fn new() -> Self {
        Self {
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute() && home.parent().is_some()),
        }
    }
}

impl Default for UnixPackages {
    fn default() -> Self {
        Self::new()
    }
}

impl PackageSystem for UnixPackages {
    fn is_macos(&self) -> bool {
        cfg!(target_os = "macos")
    }

    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        wcc_unix::process::find(name, self.home.as_deref())
    }

    fn run(&self, program: &Path, args: &[String], admin: bool) -> io::Result<Output> {
        let output = wcc_unix::process::run(program, args, admin)?;
        Ok(Output {
            success: output.success,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    /// `Info.plist` through macOS's own `plutil`, which reads both the text
    /// and the binary form. A plist holding dates or data cannot become
    /// JSON; its keys are then read one by one.
    fn bundle_info(&self, bundle: &Path) -> Option<BundleInfo> {
        if !self.is_macos() {
            return None;
        }
        let plist = bundle.join("Contents").join("Info.plist");
        if !plist.is_file() {
            return None;
        }
        let whole = plutil(&["-convert", "json", "-o", "-"], &plist);
        if let Some(info) = whole.as_deref().and_then(bundle_info_from_json) {
            return Some(info);
        }
        let key = |name: &str| {
            let is_text = plutil(&["-type", name, "-o", "-"], &plist)
                .is_some_and(|kind| kind.trim() == "string");
            is_text
                .then(|| plutil(&["-extract", name, "raw", "-o", "-"], &plist))
                .flatten()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let info = BundleInfo {
            bundle_id: key("CFBundleIdentifier"),
            name: key("CFBundleDisplayName").or_else(|| key("CFBundleName")),
            version: key("CFBundleShortVersionString").or_else(|| key("CFBundleVersion")),
        };
        (info != BundleInfo::default()).then_some(info)
    }

    fn trash(&self, paths: &[PathBuf]) -> Vec<io::Result<()>> {
        if self.is_macos() {
            paths
                .iter()
                .map(|path| crate::trash::with_finder(path))
                .collect()
        } else {
            let Some(bin) = crate::trash::home_trash(self.home.as_deref()) else {
                return paths
                    .iter()
                    .map(|_| {
                        Err(io::Error::other(
                            "there is no home folder to keep a Trash in",
                        ))
                    })
                    .collect();
            };
            paths
                .iter()
                .map(|path| crate::trash::freedesktop(&bin, path))
                .collect()
        }
    }
}

/// What `plutil` printed, when it succeeded.
fn plutil(args: &[&str], plist: &Path) -> Option<String> {
    std::process::Command::new("/usr/bin/plutil")
        .args(args)
        .arg(plist)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn bundle_info_from_json(text: &str) -> Option<BundleInfo> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some(BundleInfo {
        bundle_id: text("CFBundleIdentifier"),
        name: text("CFBundleDisplayName").or_else(|| text("CFBundleName")),
        version: text("CFBundleShortVersionString").or_else(|| text("CFBundleVersion")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundles_description_is_read_from_its_plist() {
        let info = bundle_info_from_json(
            r#"{"CFBundleIdentifier":"com.tinyspeck.slackmacgap","CFBundleName":"Slack","CFBundleShortVersionString":"4.41.97"}"#,
        )
        .unwrap();
        assert_eq!(info.bundle_id.as_deref(), Some("com.tinyspeck.slackmacgap"));
        assert_eq!(info.name.as_deref(), Some("Slack"));
        assert_eq!(info.version.as_deref(), Some("4.41.97"));
        assert!(bundle_info_from_json("[").is_none());
    }

    /// A plist with a date or data value cannot become JSON.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_plist_json_cannot_hold_is_read_key_by_key() {
        let dir = tempfile::tempdir().unwrap();
        let contents = dir.path().join("Odd.app/Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.odd</string>
<key>CFBundleName</key><string>Odd</string>
<key>Stamp</key><data>AAEC</data>
</dict></plist>"#,
        )
        .unwrap();
        let info = UnixPackages::new()
            .bundle_info(&dir.path().join("Odd.app"))
            .unwrap();
        assert_eq!(info.bundle_id.as_deref(), Some("com.example.odd"));
        assert_eq!(info.name.as_deref(), Some("Odd"));
    }

    #[test]
    fn this_machines_packages_can_be_listed() {
        let machine = UnixPackages::new();
        let (everything, _) = wu_core::packages::list(&machine, true);
        let (apps, _) = wu_core::packages::list(&machine, false);
        assert!(apps.len() <= everything.len());
    }
}
