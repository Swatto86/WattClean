//! How each package manager removes what it installed. The program and its
//! arguments are decided here; the only text from outside is the package's
//! own name, which must look like one.

use std::path::PathBuf;

use super::{Manager, Package, PackageSystem};

/// What removing a package takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// Run a package manager's command.
    Command {
        program: PathBuf,
        args: Vec<String>,
        admin: bool,
    },
    /// Move the AppImage file or application bundle to the Trash.
    Trash(PathBuf),
}

/// A package name safe to pass to a package manager: no option (`-x`), no
/// spaces or shell characters.
fn safe_name(name: &str) -> bool {
    name.len() <= 255
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '_' | '-' | ':' | '@'))
}

/// How to remove `package`; `purge` also removes its settings where the
/// package manager can (APT's purge, pacman's -Rns, Flatpak's
/// --delete-data, Snap's --purge, a cask's --zap).
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
            let mut args = vec!["uninstall", "-y", "--noninteractive"];
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
    args.push(&id);
    Ok(Removal::Command {
        program: program_path,
        args: args.into_iter().map(ToString::to_string).collect(),
        admin,
    })
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
mod tests {
    use super::*;
    use crate::packages::testing::Machine;

    fn args(removal: Result<Removal, String>) -> (Vec<String>, bool) {
        match removal.unwrap() {
            Removal::Command { args, admin, .. } => (args, admin),
            Removal::Trash(path) => panic!("trash {path:?}"),
        }
    }

    #[test]
    fn each_manager_removes_with_its_own_command() {
        let machine = Machine::new(&["apt-get", "flatpak", "brew", "snap"]);
        let apt = Package::new(Manager::Apt, "vlc", "VLC");
        assert_eq!(
            args(removal(&machine, &apt, false)),
            (vec!["-y".into(), "remove".into(), "vlc".into()], true)
        );
        assert_eq!(args(removal(&machine, &apt, true)).0[1], "purge");
        let mut flatpak = Package::new(Manager::Flatpak, "org.gimp.GIMP", "GIMP");
        flatpak.installation = Some("user".into());
        assert_eq!(
            args(removal(&machine, &flatpak, true)).0,
            [
                "uninstall",
                "-y",
                "--noninteractive",
                "--user",
                "--delete-data",
                "org.gimp.GIMP"
            ]
        );
        let cask = Package::new(Manager::HomebrewCask, "firefox", "Firefox");
        assert_eq!(
            args(removal(&machine, &cask, true)),
            (
                vec![
                    "uninstall".into(),
                    "--cask".into(),
                    "--zap".into(),
                    "firefox".into()
                ],
                false
            )
        );
        let pacman = Package::new(Manager::Pacman, "vlc", "VLC");
        assert!(
            removal(&machine, &pacman, false).is_err(),
            "pacman is not installed"
        );
    }

    #[test]
    fn names_that_could_be_options_are_refused() {
        let machine = Machine::new(&["apt-get"]);
        for id in ["-y", "a b", "x;reboot", "$(id)", ""] {
            let package = Package::new(Manager::Apt, id, id);
            assert!(removal(&machine, &package, false).is_err(), "{id:?}");
        }
    }

    /// A removal run as administrator never takes its program from a folder
    /// the account (or anything it runs) can write; Homebrew's, run as the
    /// account, still can.
    #[test]
    fn an_administrator_removal_never_runs_a_program_the_account_could_have_planted() {
        let mut machine = Machine::new(&["port", "brew", "apt-get", "dpkg"]);
        machine
            .untrusted
            .extend(["port", "brew", "apt-get"].map(String::from));
        let port = Package::new(Manager::MacPorts, "ffmpeg", "ffmpeg");
        assert!(removal(&machine, &port, false).is_err());
        let brew = Package::new(Manager::Homebrew, "wget", "wget");
        assert!(removal(&machine, &brew, false).is_ok());
        let apt = Package::new(Manager::Apt, "vlc", "VLC");
        match removal(&machine, &apt, false) {
            Ok(Removal::Command { program, admin, .. }) => {
                assert_eq!((program, admin), ("/usr/bin/dpkg".into(), true));
            }
            other => panic!("{other:?}"),
        }
    }

    // Unix paths: on Windows `/Applications` is not a full path.
    #[cfg(unix)]
    #[test]
    fn bundles_and_appimages_go_to_the_trash_whole() {
        let machine = Machine::new(&[]);
        let app = Package::new(Manager::MacApp, "/Applications/Slack.app", "Slack");
        assert_eq!(
            removal(&machine, &app, false),
            Ok(Removal::Trash("/Applications/Slack.app".into()))
        );
        let odd = Package::new(Manager::MacApp, "/Applications/../etc", "x");
        assert!(removal(&machine, &odd, false).is_err());
        let relative = Package::new(Manager::AppImage, "Krita.AppImage", "Krita");
        assert!(removal(&machine, &relative, false).is_err());
    }
}
