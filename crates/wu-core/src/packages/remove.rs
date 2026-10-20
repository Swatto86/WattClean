//! How each package manager removes what it installed. The program and its
//! arguments are decided here; the only text from outside is the package's
//! own name, which must look like one.

use std::path::{Path, PathBuf};

use super::{Manager, Output, Package, PackageSystem};

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
    match program {
        "apt-get" => goes_alone(
            system,
            package,
            program,
            &program_path,
            &["-s", if purge { "purge" } else { "remove" }],
            apt_takes,
        )?,
        "dnf" | "zypper" => {
            let rpm = system.find_admin_program("rpm").ok_or_else(|| {
                format!(
                    "rpm is not installed, so WattClean cannot check what else removing {} would remove",
                    package.name
                )
            })?;
            // dnf and zypper remove every installed copy of the name (both
            // architectures); rpm tests them all only when told to.
            let test = ["-e", "--test", "--allmatches"];
            goes_alone(system, package, program, &rpm, &test, rpm_takes)?;
        }
        _ => {}
    }
    args.push(&id);
    Ok(Removal::Command {
        program: program_path,
        args: args.into_iter().map(ToString::to_string).collect(),
        admin,
    })
}

/// Asks `checker` (with `args` and the package's name, without
/// administrator rights) what removing `package` with `remover` would take;
/// an error naming the others when anything besides the package would go.
fn goes_alone(
    system: &dyn PackageSystem,
    package: &Package,
    remover: &str,
    checker: &Path,
    args: &[&str],
    read: fn(&Output) -> Option<Vec<String>>,
) -> Result<(), String> {
    let args: Vec<String> = args
        .iter()
        .copied()
        .chain([package.id.as_str()])
        .map(ToString::to_string)
        .collect();
    let output = system.run(checker, &args, false).map_err(|error| {
        format!(
            "WattClean could not check what else removing {} would remove: {error}",
            package.name
        )
    })?;
    let Some(mut others) = read(&output) else {
        let said = output
            .last_words()
            .unwrap_or("its package manager gave no answer");
        return Err(format!(
            "WattClean could not check what else removing {} would remove, so it was left alone: {said}",
            package.name
        ));
    };
    // `name:arch` is the chosen package's own copy for another architecture.
    others.retain(|other| other.split(':').next() != Some(package.id.as_str()));
    others.sort();
    others.dedup();
    if others.is_empty() {
        return Ok(());
    }
    const NAMED: usize = 5;
    let mut list = others
        .iter()
        .take(NAMED)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if others.len() > NAMED {
        list.push_str(&format!(" and {} more", others.len() - NAMED));
    }
    let (count, verb) = match others.len() {
        1 => ("1 other package".to_string(), "needs"),
        n => (format!("{n} other packages"), "need"),
    };
    Err(format!(
        "Removing {} would also remove {count} that {verb} it: {list}. WattClean removes only the program chosen; if you mean to remove them all, do it with {remover} yourself.",
        package.name
    ))
}

/// The packages `apt-get -s remove|purge` lists (`Remv name [version]`,
/// `Purg …`); `None` when the simulation failed.
fn apt_takes(output: &Output) -> Option<Vec<String>> {
    output.success.then(|| {
        output
            .stdout
            .lines()
            .chain(output.stderr.lines())
            .filter_map(|line| {
                line.strip_prefix("Remv ")
                    .or_else(|| line.strip_prefix("Purg "))
            })
            .filter_map(|rest| rest.split_whitespace().next())
            .map(ToString::to_string)
            .collect()
    })
}

/// The packages `rpm -e --test` says need the one asked about (`… is
/// needed by (installed) name-version-release.arch`); none when it would
/// succeed, `None` when it failed for another reason.
fn rpm_takes(output: &Output) -> Option<Vec<String>> {
    if output.success {
        return Some(Vec::new());
    }
    let needing: Vec<String> = output
        .stderr
        .lines()
        .chain(output.stdout.lines())
        .filter_map(|line| line.rsplit_once(" is needed by "))
        .map(|(_, by)| by.trim().trim_start_matches("(installed)").trim())
        .filter(|nevra| !nevra.is_empty())
        .map(|nevra| {
            // The version and release hold no `-`; the name may.
            let mut parts = nevra.rsplitn(3, '-');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(_), Some(_), Some(name)) => name.to_string(),
                _ => nevra.to_string(),
            }
        })
        .collect();
    (!needing.is_empty()).then_some(needing)
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
