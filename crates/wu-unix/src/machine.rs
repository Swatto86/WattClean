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
    /// and the binary form.
    fn bundle_info(&self, bundle: &Path) -> Option<BundleInfo> {
        if !self.is_macos() {
            return None;
        }
        let plist = bundle.join("Contents").join("Info.plist");
        let output = std::process::Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(&plist)
            .stdin(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())?;
        bundle_info_from_json(&String::from_utf8_lossy(&output.stdout))
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

    #[test]
    fn this_machines_packages_can_be_listed() {
        let machine = UnixPackages::new();
        let (everything, _) = wu_core::packages::list(&machine, true);
        let (apps, _) = wu_core::packages::list(&machine, false);
        assert!(apps.len() <= everything.len());
    }
}
