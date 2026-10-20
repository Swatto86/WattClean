//! How each package manager removes what it installed, and what stops a
//! program's background jobs first. The program and its arguments are
//! decided here; the only text from outside is the package's own name and
//! a job's label, which must look like names.

use std::path::{Path, PathBuf};

use super::launchd::Domain;
use super::others::takes_nothing_else;
use super::{Leftover, Manager, Package, PackageSystem, launchd};

/// One command a removal runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub admin: bool,
}

/// What removing a package takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// Run a package manager's command, after the steps `before` it
    /// (stopping a background service), in order: nothing more runs once
    /// one fails.
    Command {
        program: PathBuf,
        args: Vec<String>,
        admin: bool,
        before: Vec<Step>,
        /// sudo, run by Homebrew on macOS, asks for an administrator's
        /// password through WattClean's own password box (`SUDO_ASKPASS`):
        /// an app has no terminal for it to ask in.
        askpass: bool,
    },
    /// Move the AppImage file or application bundle to the Trash.
    Trash(PathBuf),
}

/// A package name safe to pass to a package manager: no option (`-x`), no
/// spaces or shell characters.
pub(super) fn safe_name(name: &str) -> bool {
    name.len() <= 255
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '_' | '-' | ':' | '@'))
}

/// How to remove `package`; `purge` also removes its settings where the
/// package manager can (APT's purge, pacman's -Rns, Flatpak's
/// --delete-data, Snap's --purge, a cask's --zap).
///
/// APT, dnf and zypper, told yes as they are here, also remove every
/// package that needs the one chosen. Before those, the package manager is
/// asked (without administrator rights) what the removal would take, and
/// it is refused when anything else would go or the answer cannot be read.
/// dpkg, rpm and pacman refuse such a removal themselves.
pub fn removal(
    system: &dyn PackageSystem,
    package: &Package,
    purge: bool,
) -> Result<Removal, String> {
    if !matches!(package.manager, Manager::AppImage | Manager::MacApp) && !safe_name(&package.id) {
        return Err(format!(
            "{:?} is not a package name WattClean will pass on",
            package.id
        ));
    }
    // An app moved while it is open runs on from the Trash, and writes its
    // folders again after they were offered as leftovers.
    if let Some(bundle) = package.bundle()
        && !system.running_from(bundle).is_empty()
    {
        return Err(format!(
            "{} is open: quit it, then uninstall.",
            package.name
        ));
    }
    let id = package.id.clone();
    // Run as administrator, a program comes only from where only an
    // administrator can change it.
    let found = |program: &str| system.find_admin_program(program).is_some();
    let (program, mut args, admin): (&str, Vec<&str>, bool) = match package.manager {
        Manager::Apt if found("apt-get") => (
            "apt-get",
            vec!["-y", if purge { "purge" } else { "remove" }],
            true,
        ),
        Manager::Apt => (
            "dpkg",
            vec![if purge { "--purge" } else { "--remove" }],
            true,
        ),
        Manager::Rpm if found("dnf") => ("dnf", vec!["-y", "remove"], true),
        Manager::Rpm if found("zypper") => ("zypper", vec!["--non-interactive", "remove"], true),
        Manager::Rpm => ("rpm", vec!["-e"], true),
        Manager::Pacman => (
            "pacman",
            vec![if purge { "-Rns" } else { "-R" }, "--noconfirm"],
            true,
        ),
        Manager::Flatpak => {
            // Not --noninteractive: its quiet mode tells polkit not to ask
            // for a password, so a system-wide app could not be removed by
            // an account polkit does not let through without one.
            let mut args = vec!["uninstall", "-y"];
            match package.installation.as_deref() {
                Some("user") => args.push("--user"),
                Some("system") => args.push("--system"),
                _ => {}
            }
            if purge {
                args.push("--delete-data");
            }
            // Flatpak asks for administrator rights itself when it needs them.
            ("flatpak", args, false)
        }
        Manager::Snap => (
            "snap",
            if purge {
                vec!["remove", "--purge"]
            } else {
                vec!["remove"]
            },
            true,
        ),
        // Homebrew refuses to run as administrator.
        Manager::Homebrew => ("brew", vec!["uninstall"], false),
        Manager::HomebrewCask => (
            "brew",
            if purge {
                vec!["uninstall", "--cask", "--zap"]
            } else {
                vec!["uninstall", "--cask"]
            },
            false,
        ),
        Manager::MacPorts => ("port", vec!["uninstall"], true),
        Manager::AppImage | Manager::MacApp => return trash_target(package),
    };
    let program_path = if admin {
        system.find_admin_program(program)
    } else {
        system.find_program(program)
    };
    let program_path = program_path.ok_or_else(|| {
        format!(
            "{program} is not installed, so {} cannot be removed here",
            package.name
        )
    })?;
    takes_nothing_else(system, package, program, &program_path, purge)?;
    let before = match package.manager {
        Manager::Homebrew if system.is_macos() => services(system, package, &program_path)?,
        _ => Vec::new(),
    };
    args.push(&id);
    Ok(Removal::Command {
        program: program_path,
        args: args.into_iter().map(ToString::to_string).collect(),
        admin,
        before,
        askpass: system.is_macos()
            && matches!(package.manager, Manager::Homebrew | Manager::HomebrewCask),
    })
}

/// What stops a Homebrew formula's background service before it goes:
/// `brew uninstall` leaves it running from a deleted folder, and launchd
/// starts it again at every login or boot. The account's own service
/// (`~/Library/LaunchAgents`) is stopped by `brew services stop`, which
/// also deletes its plist. One that runs for every account
/// (`/Library/LaunchDaemons`, started with sudo) is booted out of launchd
/// as administrator, since Homebrew's own command will not run as
/// administrator from an app; its plist is then offered as a leftover.
/// While root's plist is there Homebrew may refuse to stop the account's
/// own (it does when launchd has not loaded it), which is then left to the
/// leftovers too, stopped when it moves. A job launchd has not
/// loaded needs no stopping.
fn services(
    system: &dyn PackageSystem,
    package: &Package,
    brew: &Path,
) -> Result<Vec<Step>, String> {
    let label = launchd::service_label(&package.id);
    let plist = format!("{label}.plist");
    let exists = |path: &Path| std::fs::symlink_metadata(path).is_ok();
    if exists(&system.at(&format!("/Library/LaunchDaemons/{plist}"))) {
        let launchctl = system.find_admin_program("launchctl").ok_or_else(|| {
            format!(
                "launchctl is not found, so {}'s background service cannot be stopped",
                package.name
            )
        })?;
        let target = format!("system/{label}");
        if !loaded(system, &launchctl, &target) {
            return Ok(Vec::new());
        }
        return Ok(vec![Step {
            program: launchctl,
            args: vec!["bootout".into(), target],
            admin: true,
        }]);
    }
    let mine = system
        .home()
        .map(|home| home.join("Library/LaunchAgents").join(&plist));
    Ok(mine
        .filter(|path| exists(path))
        .map(|_| Step {
            program: brew.to_path_buf(),
            args: vec!["services".into(), "stop".into(), package.id.clone()],
            admin: false,
        })
        .into_iter()
        .collect())
}

/// Whether launchd has the job `target` (`gui/501/<label>`) loaded; asked
/// without administrator rights.
fn loaded(system: &dyn PackageSystem, launchctl: &Path, target: &str) -> bool {
    let args = ["print".to_string(), target.to_string()];
    system
        .run(launchctl, &args, false, &[])
        .is_ok_and(|output| output.success)
}

/// What stops a leftover's background job before its plist (or the helper
/// a daemon starts) goes to the Trash: launchd runs a trashed plist's job
/// on until logout or shutdown. launchd's bootout by label, and only while
/// launchd has it loaded: from the account's own session without
/// administrator rights, or a daemon for every account (`system`) with
/// them. `None` when there is nothing to stop. A label that could be an
/// option or a path, or WattClean's own, is refused.
pub fn stop_job(system: &dyn PackageSystem, leftover: &Leftover) -> Result<Option<Step>, String> {
    let Some(job) = &leftover.job else {
        return Ok(None);
    };
    let label = &job.label;
    if !safe_name(label) || launchd::is_own(label) {
        return Err(format!(
            "its background job {label:?} is not one WattClean will stop"
        ));
    }
    let launchctl = system
        .find_admin_program("launchctl")
        .ok_or("launchctl is not found, so its background job cannot be stopped")?;
    let target = match job.domain {
        Domain::Session => {
            let uid = system.uid().ok_or(
                "this account's user id is unknown, so its background job cannot be stopped",
            )?;
            format!("gui/{uid}/{label}")
        }
        Domain::System => format!("system/{label}"),
    };
    if !loaded(system, &launchctl, &target) {
        return Ok(None);
    }
    Ok(Some(Step {
        program: launchctl,
        args: vec!["bootout".into(), target],
        admin: job.domain == Domain::System,
    }))
}

/// An AppImage or app bundle goes to the Trash whole, if it is one.
fn trash_target(package: &Package) -> Result<Removal, String> {
    let path = PathBuf::from(&package.id);
    let extension = match package.manager {
        Manager::AppImage => "appimage",
        _ => "app",
    };
    let fits = path.is_absolute()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
    if fits {
        Ok(Removal::Trash(path))
    } else {
        Err(format!(
            "{} is not an application WattClean can remove",
            path.display()
        ))
    }
}

#[cfg(test)]
#[path = "remove_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "remove_mac_tests.rs"]
mod mac_tests;
