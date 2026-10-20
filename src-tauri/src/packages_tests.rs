use std::io;
use std::path::{Path, PathBuf};

use wu_core::packages::{BundleInfo, Manager, Output};

use super::*;

/// A Mac whose Homebrew fails a cask's removal at its sudo step: the
/// progress on stdout, the error on stderr.
struct SudoFails;

impl PackageSystem for SudoFails {
    fn is_macos(&self) -> bool {
        true
    }
    fn home(&self) -> Option<PathBuf> {
        None
    }
    fn find_program(&self, name: &str) -> Option<PathBuf> {
        Some(Path::new("/opt/homebrew/bin").join(name))
    }
    fn find_admin_program(&self, _name: &str) -> Option<PathBuf> {
        None
    }
    fn run(
        &self,
        _program: &Path,
        _args: &[String],
        _admin: bool,
        _env: &[(&str, &str)],
    ) -> io::Result<Output> {
        Ok(Output {
            success: false,
            stdout: "==> Uninstalling Cask battle-net\n==> Removing files:\n/Applications/Battle.net.app\n".into(),
            stderr: "Error: Failure while executing; `/usr/bin/sudo -u root -- /bin/rm -f -r -- /Applications/Battle.net.app` exited with 1. Here's the output:\nsudo: a terminal is required to read the password\nsudo: a password is required\n".into(),
        })
    }
    fn bundle_info(&self, _bundle: &Path) -> Option<BundleInfo> {
        None
    }
    fn trash(&self, paths: &[PathBuf]) -> Vec<io::Result<()>> {
        paths
            .iter()
            .map(|_| Err(io::Error::other("not here")))
            .collect()
    }
    fn reveal(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }
    fn askpass(&self) -> io::Result<PathBuf> {
        Ok(PathBuf::from("/WattClean/uninstall/askpass.sh"))
    }
}

#[test]
fn a_failed_removal_reports_the_package_managers_error_not_its_progress() {
    let cask = Package::new(Manager::HomebrewCask, "battle-net", "Blizzard Battle.net");
    let Err(error) = remove(&SudoFails, &cask, false) else {
        panic!("the removal should have failed");
    };
    assert!(
        error.message.starts_with(
            "Blizzard Battle.net could not be removed: Error: Failure while executing;"
        ),
        "{}",
        error.message
    );
}

/// A Mac that records each command run (the variables it was given first,
/// then `admin` when it asked for rights) and fails the ones named.
struct Mac {
    fails: Vec<&'static str>,
    ran: Mutex<Vec<String>>,
    trashed: Mutex<Vec<PathBuf>>,
    revealed: Mutex<Vec<PathBuf>>,
}

const ASKPASS: &str = "/WattClean/uninstall/askpass.sh";

impl Mac {
    fn failing(fails: &[&'static str]) -> Self {
        Self {
            fails: fails.to_vec(),
            ran: Mutex::new(Vec::new()),
            trashed: Mutex::new(Vec::new()),
            revealed: Mutex::new(Vec::new()),
        }
    }

    fn ran(&self) -> Vec<String> {
        lock(&self.ran).clone()
    }
}

impl PackageSystem for Mac {
    fn is_macos(&self) -> bool {
        true
    }
    fn home(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/Users/t"))
    }
    fn find_program(&self, name: &str) -> Option<PathBuf> {
        Some(Path::new("/opt/homebrew/bin").join(name))
    }
    fn find_admin_program(&self, name: &str) -> Option<PathBuf> {
        Some(Path::new("/bin").join(name))
    }
    fn run(
        &self,
        program: &Path,
        args: &[String],
        admin: bool,
        env: &[(&str, &str)],
    ) -> io::Result<Output> {
        let mut command = program.file_name().unwrap().to_string_lossy().to_string();
        for arg in args {
            command.push(' ');
            command.push_str(arg);
        }
        let mut line: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        if admin {
            line.push("admin".into());
        }
        line.push(command.clone());
        lock(&self.ran).push(line.join(" "));
        let fails = self.fails.contains(&command.as_str());
        Ok(Output {
            success: !fails,
            stdout: String::new(),
            stderr: if fails {
                "Error: it failed".into()
            } else {
                String::new()
            },
        })
    }
    fn bundle_info(&self, _bundle: &Path) -> Option<BundleInfo> {
        None
    }
    fn trash(&self, paths: &[PathBuf]) -> Vec<io::Result<()>> {
        lock(&self.trashed).extend_from_slice(paths);
        paths.iter().map(|_| Ok(())).collect()
    }
    fn uid(&self) -> Option<u32> {
        Some(501)
    }
    fn reveal(&self, path: &Path) -> io::Result<()> {
        lock(&self.revealed).push(path.to_path_buf());
        Ok(())
    }
    fn askpass(&self) -> io::Result<PathBuf> {
        Ok(PathBuf::from(ASKPASS))
    }
}

fn uninstall_tailscale(askpass: bool) -> Removal {
    Removal::Command {
        program: "/opt/homebrew/bin/brew".into(),
        args: vec!["uninstall".into(), "tailscale".into()],
        admin: false,
        before: vec![Step {
            program: "/bin/launchctl".into(),
            args: vec!["bootout".into(), "system/homebrew.mxcl.tailscale".into()],
            admin: true,
        }],
        askpass,
    }
}

/// Homebrew's sudo asks for a password through WattClean's password box,
/// after the formula's service was stopped; a service that cannot be
/// stopped leaves the formula installed.
#[test]
fn a_service_is_stopped_first_and_homebrew_asks_through_wattclean() {
    let mac = Mac::failing(&[]);
    carry_out(&mac, "tailscale", uninstall_tailscale(true)).unwrap();
    assert_eq!(
        mac.ran(),
        [
            "admin launchctl bootout system/homebrew.mxcl.tailscale".to_string(),
            format!("SUDO_ASKPASS={ASKPASS} brew uninstall tailscale"),
        ]
    );
    let plain = Mac::failing(&[]);
    carry_out(&plain, "tailscale", uninstall_tailscale(false)).unwrap();
    assert_eq!(plain.ran()[1], "brew uninstall tailscale");

    let stuck = Mac::failing(&["launchctl bootout system/homebrew.mxcl.tailscale"]);
    let error = carry_out(&stuck, "tailscale", uninstall_tailscale(true)).unwrap_err();
    assert_eq!(
        error.message,
        "tailscale's background service could not be stopped, so it was left installed: Error: it failed"
    );
    assert_eq!(stuck.ran().len(), 1, "brew never ran: {:?}", stuck.ran());
}

fn leftover(path: &str, job: Option<&str>) -> Leftover {
    Leftover {
        path: path.into(),
        size: Some(1),
        folder: job.is_none(),
        job: job.map(String::from),
    }
}

/// A plist moved to the Trash leaves its job running until logout, so a
/// loaded job is stopped first; one that cannot be stopped stays where it
/// is, and WattClean's own is never stopped.
#[test]
fn a_loaded_job_is_stopped_before_its_plist_goes_to_the_trash() {
    let mac =
        Mac::failing(&["launchctl bootout gui/501 /Users/t/Library/LaunchAgents/Stuck.plist"]);
    let chosen = [
        leftover("/Users/t/Library/LaunchAgents/Tea.plist", Some("Tea")),
        leftover("/Users/t/Library/LaunchAgents/Stuck.plist", Some("Stuck")),
        leftover("/Users/t/Library/Application Support/Tea", None),
        leftover(
            "/Users/t/Library/LaunchAgents/co.swatto.wattclean.plist",
            Some("co.swatto.wattclean"),
        ),
    ];
    let trashed = trash_chosen(&mac, &chosen);
    assert_eq!(trashed.moved, 2);
    assert_eq!(trashed.failed.len(), 2, "{:?}", trashed.failed);
    assert!(
        trashed.failed[0].starts_with(
            "/Users/t/Library/LaunchAgents/Stuck.plist: its background job could not be stopped"
        ),
        "{:?}",
        trashed.failed
    );
    assert_eq!(
        mac.ran(),
        [
            "launchctl print gui/501/Tea",
            "launchctl bootout gui/501 /Users/t/Library/LaunchAgents/Tea.plist",
            "launchctl print gui/501/Stuck",
            "launchctl bootout gui/501 /Users/t/Library/LaunchAgents/Stuck.plist",
        ]
    );
    assert_eq!(
        *lock(&mac.trashed),
        [
            PathBuf::from("/Users/t/Library/LaunchAgents/Tea.plist"),
            PathBuf::from("/Users/t/Library/Application Support/Tea"),
        ]
    );
}

#[test]
fn a_leftover_is_shown_by_its_number_only() {
    let mac = Mac::failing(&[]);
    let leftovers = [
        leftover("/Users/t/.config/tea", None),
        leftover("/Users/t/Library/Caches/tea", None),
    ];
    reveal(&mac, &leftovers, 1).unwrap();
    assert_eq!(
        *lock(&mac.revealed),
        [PathBuf::from("/Users/t/Library/Caches/tea")]
    );
    let gone = reveal(&mac, &leftovers, 2).unwrap_err();
    assert_eq!(gone.message, "That leftover is no longer listed.");
}
