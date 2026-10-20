//! This machine's package managers and application bundles.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use wu_core::packages::{BundleInfo, LaunchdJob, Output, PackageSystem, within};

pub struct UnixPackages {
    home: Option<PathBuf>,
    /// WattClean's own folder for the uninstaller, where the password box
    /// Homebrew's sudo asks through is written.
    data: Option<PathBuf>,
}

impl UnixPackages {
    pub fn new() -> Self {
        Self {
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute() && home.parent().is_some()),
            data: None,
        }
    }

    /// Keeps what the uninstaller writes in `folder`.
    #[must_use]
    pub fn with_data(mut self, folder: PathBuf) -> Self {
        self.data = Some(folder);
        self
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

    fn find_admin_program(&self, name: &str) -> Option<PathBuf> {
        wcc_unix::process::find_trusted(name)
    }

    fn run(
        &self,
        program: &Path,
        args: &[String],
        admin: bool,
        env: &[(&str, &str)],
    ) -> io::Result<Output> {
        let output = wcc_unix::process::run(program, args, admin, env)?;
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

    /// `ps -axo comm=` prints each program's full path on macOS. Only apps
    /// (macOS's) are asked about; on Linux `ps` prints names, not paths.
    fn running_from(&self, folder: &Path) -> Vec<String> {
        if !self.is_macos() {
            return Vec::new();
        }
        let listing = wcc_unix::process::quick(
            Path::new("/bin/ps"),
            &["-axo", "comm="],
            Duration::from_secs(10),
        );
        listing
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|path| within(path, folder))
            .map(str::to_string)
            .collect()
    }

    /// The plist through `plutil`, as `Info.plist` is read.
    fn launchd_job(&self, plist: &Path) -> Option<LaunchdJob> {
        if !self.is_macos() {
            return None;
        }
        plutil(&["-convert", "json", "-o", "-"], plist)
            .as_deref()
            .and_then(LaunchdJob::from_json)
    }

    fn uid(&self) -> Option<u32> {
        Some(wcc_unix::uid())
    }

    fn askpass(&self) -> io::Result<PathBuf> {
        let folder = self
            .data
            .as_deref()
            .filter(|_| self.is_macos())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "there is no password box for Homebrew here",
                )
            })?;
        crate::askpass::write(folder)
    }

    /// Finder's `open -R` selects the item in its folder. On Linux the
    /// folder holding it opens with the desktop's own file manager through
    /// `xdg-open`, which may wait for the file manager to close: it is left
    /// running, and reaped when it ends.
    fn reveal(&self, path: &Path) -> io::Result<()> {
        if self.is_macos() {
            let args = [String::from("-R"), path.to_string_lossy().into_owned()];
            let output = wcc_unix::process::run(Path::new("/usr/bin/open"), &args, false, &[])?;
            return if output.success {
                Ok(())
            } else {
                Err(io::Error::other(output.stderr.trim().to_string()))
            };
        }
        let folder = path
            .parent()
            .filter(|folder| folder.is_absolute())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "it has no folder"))?;
        let opener = self
            .find_program("xdg-open")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "xdg-open is not installed"))?;
        let mut child = std::process::Command::new(opener)
            .arg(folder)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        std::thread::spawn(move || child.wait());
        Ok(())
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

    /// This test runs from its own folder, which a longer name next to it
    /// does not hold.
    #[cfg(target_os = "macos")]
    #[test]
    fn programs_running_from_a_folder_are_found_by_their_whole_path() {
        let exe = std::env::current_exe().unwrap();
        let folder = exe.parent().unwrap();
        let running = UnixPackages::new().running_from(folder);
        assert!(
            running.iter().any(|path| Path::new(path) == exe),
            "{} in {running:?}",
            exe.display()
        );
        let longer = PathBuf::from(format!("{}x", folder.display()));
        assert_eq!(
            UnixPackages::new().running_from(&longer),
            Vec::<String>::new()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_launch_agents_plist_is_read_through_plutil() {
        let dir = tempfile::tempdir().unwrap();
        let plist = dir.path().join("Tea.plist");
        std::fs::write(
            &plist,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>Tea</string>
<key>ProgramArguments</key><array><string>/Applications/Tea.app/Contents/MacOS/tea</string></array>
<key>RunAtLoad</key><true/>
</dict></plist>"#,
        )
        .unwrap();
        let job = UnixPackages::new().launchd_job(&plist).unwrap();
        assert_eq!(job.label, "Tea");
        assert_eq!(
            job.program.as_deref(),
            Some("/Applications/Tea.app/Contents/MacOS/tea")
        );
    }

    #[test]
    fn this_machines_packages_can_be_listed() {
        let machine = UnixPackages::new();
        let (everything, _) = wu_core::packages::list(&machine, true);
        let (apps, _) = wu_core::packages::list(&machine, false);
        assert!(apps.len() <= everything.len());
    }
}
