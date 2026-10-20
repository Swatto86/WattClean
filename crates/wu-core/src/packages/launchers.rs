//! Which of the system's packages are applications: those that installed a
//! visible launcher (`.desktop` file) in `/usr/share/applications`, named
//! by it. The system's package manager says which package owns each one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{Manager, Package, PackageSystem};

/// The launchers (`.desktop` files) the system's packages installed, by
/// package, with the name each launcher shows.
pub(super) fn mark_apps(system: &dyn PackageSystem, packages: &mut [Package]) {
    let folder = system.at("/usr/share/applications");
    let launchers: Vec<PathBuf> = std::fs::read_dir(&folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                // Only files: rpm prints nothing for one it cannot read.
                .filter(|p| p.extension().is_some_and(|ext| ext == "desktop") && p.is_file())
                .collect()
        })
        .unwrap_or_default();
    if launchers.is_empty() {
        return;
    }
    let owners = owners(system, &launchers, &folder);
    for package in packages
        .iter_mut()
        .filter(|p| matches!(p.manager, Manager::Apt | Manager::Rpm | Manager::Pacman))
    {
        // A hidden launcher (a helper the menu does not show) is no app.
        if let Some(name) = owners.get(&package.id).and_then(|l| launcher_name(l)) {
            package.app = true;
            package.name = name;
        }
    }
}

/// Which package owns each launcher, asked of the system's package manager;
/// a launcher no package owns is left out. Each answer is matched to the
/// launcher it names, never to the line it is on: pacman prints nothing for
/// an unowned file and rpm a line for each of a file's owners.
fn owners(
    system: &dyn PackageSystem,
    launchers: &[PathBuf],
    folder: &Path,
) -> HashMap<String, PathBuf> {
    // The package manager knows the real paths, not the sandbox's.
    let real: Vec<String> = launchers
        .iter()
        .filter_map(|p| p.file_name())
        .map(|name| format!("/usr/share/applications/{}", name.to_string_lossy()))
        .collect();
    // dpkg, rpm and pacman print what they know even when one file is unowned.
    let ask = |program: &Path, first: &[&str], files: &[String]| {
        let args: Vec<String> = first
            .iter()
            .map(ToString::to_string)
            .chain(files.iter().cloned())
            .collect();
        system
            .run(program, &args, false, &[])
            .map(|output| output.stdout)
            .unwrap_or_default()
    };
    let owned = if let Some(dpkg) = system.find_program("dpkg-query") {
        parse_dpkg_owners(&ask(&dpkg, &["-S"], &real))
    } else if let Some(rpm) = system.find_program("rpm") {
        rpm_owners(&real, |files| {
            ask(&rpm, &["-qf", "--queryformat", r"%{NAME}\n"], files)
        })
    } else if let Some(pacman) = system.find_program("pacman") {
        parse_pacman_owners(&ask(&pacman, &["-Qo"], &real))
    } else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for (name, path) in owned {
        if let Some(file) = Path::new(&path).file_name() {
            out.entry(name).or_insert_with(|| folder.join(file));
        }
    }
    out
}

/// `package[:arch][, other]: /usr/share/applications/x.desktop`, as
/// (package, path) pairs; `diversion by …` lines name no owner.
fn parse_dpkg_owners(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once(": "))
        .flat_map(|(names, path)| {
            names.split(", ").filter_map(move |name| {
                let name = name.split(':').next().unwrap_or(name).trim();
                (!name.is_empty() && !name.contains(' '))
                    .then(|| (name.to_string(), path.trim().to_string()))
            })
        })
        .collect()
}

/// `/usr/share/applications/x.desktop is owned by package 1.0-1`, as
/// (package, path) pairs; pacman tells of unowned files on stderr.
fn parse_pacman_owners(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.rsplit_once(" is owned by "))
        .filter_map(|(path, owner)| {
            let name = owner.split_whitespace().next()?;
            (!path.trim().is_empty()).then(|| (name.to_string(), path.trim().to_string()))
        })
        .collect()
}

/// `rpm -qf` names each file's owners one per line, or says with spaces
/// that none owns it, but never names the file. Its lines are matched to
/// the files in order only when there is one per file; otherwise each
/// file is asked about alone.
fn rpm_owners(files: &[String], ask: impl Fn(&[String]) -> String) -> Vec<(String, String)> {
    let owner = |line: &str| {
        let name = line.trim();
        (!name.is_empty() && !name.contains(' ')).then(|| name.to_string())
    };
    let text = ask(files);
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() == files.len() {
        return lines
            .iter()
            .zip(files)
            .filter_map(|(line, file)| owner(line).map(|name| (name, file.clone())))
            .collect();
    }
    files
        .iter()
        .flat_map(|file| {
            ask(std::slice::from_ref(file))
                .lines()
                .filter_map(owner)
                .map(|name| (name, file.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A launcher's `Name=` in its main section, unless it is hidden.
fn launcher_name(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut name = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if line == "NoDisplay=true" || line == "Hidden=true" {
            return None;
        }
        if let Some(value) = line.strip_prefix("Name=") {
            name.get_or_insert_with(|| value.trim().to_string());
        }
    }
    name.filter(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::testing::Machine;

    fn launchers(machine: &Machine, files: &[&str]) -> (Vec<PathBuf>, PathBuf) {
        let folder = machine.at("/usr/share/applications");
        (files.iter().map(|file| folder.join(file)).collect(), folder)
    }

    #[test]
    fn owner_listings_are_read_as_package_and_path() {
        let pair = |name: &str, path: &str| (name.to_string(), path.to_string());
        assert_eq!(
            parse_dpkg_owners(
                "vlc-bin, vlc: /usr/share/applications/vlc.desktop\n\
                 libreoffice-common:amd64, libreoffice-core:i386: /usr/share/applications/libreoffice-startcenter.desktop\n\
                 diversion by firefox-esr from: /usr/share/applications/firefox.desktop\n"
            ),
            [
                pair("vlc-bin", "/usr/share/applications/vlc.desktop"),
                pair("vlc", "/usr/share/applications/vlc.desktop"),
                pair(
                    "libreoffice-common",
                    "/usr/share/applications/libreoffice-startcenter.desktop"
                ),
                pair(
                    "libreoffice-core",
                    "/usr/share/applications/libreoffice-startcenter.desktop"
                ),
            ]
        );
        assert_eq!(
            parse_pacman_owners(
                "/usr/share/applications/gimp.desktop is owned by gimp 2.10.38-1\n\
                 /usr/share/applications/org.inkscape.Inkscape.desktop is owned by inkscape 1:1.3.2-7\n\
                 error: No package owns /usr/share/applications/vmware.desktop\n"
            ),
            [
                pair("gimp", "/usr/share/applications/gimp.desktop"),
                pair(
                    "inkscape",
                    "/usr/share/applications/org.inkscape.Inkscape.desktop"
                ),
            ]
        );
    }

    /// pacman prints nothing on stdout for a launcher no package owns, so its
    /// answers are matched to launchers by the path each one names.
    #[test]
    fn pacman_owners_are_matched_by_path_past_an_unowned_launcher() {
        let machine = Machine::new(&["pacman"]);
        let (asked, folder) = launchers(
            &machine,
            &[
                "vmware.desktop",
                "gimp.desktop",
                "org.inkscape.Inkscape.desktop",
            ],
        );
        machine.replies(
            "pacman -Qo /usr/share/applications/vmware.desktop /usr/share/applications/gimp.desktop /usr/share/applications/org.inkscape.Inkscape.desktop",
            false,
            "/usr/share/applications/gimp.desktop is owned by gimp 2.10.38-1\n\
             /usr/share/applications/org.inkscape.Inkscape.desktop is owned by inkscape 1:1.3.2-7\n",
            "error: No package owns /usr/share/applications/vmware.desktop\n",
        );
        let found = owners(&machine, &asked, &folder);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found["gimp"], folder.join("gimp.desktop"));
        assert_eq!(
            found["inkscape"],
            folder.join("org.inkscape.Inkscape.desktop")
        );
    }

    /// rpm prints a line per owner, so a launcher two packages own shifts the
    /// batch's lines; each launcher is then asked about alone.
    #[test]
    fn rpm_owners_are_asked_one_by_one_when_the_lines_do_not_match() {
        let machine = Machine::new(&["rpm"]);
        let (asked, folder) = launchers(
            &machine,
            &[
                "wine.desktop",
                "gimp.desktop",
                "org.inkscape.Inkscape.desktop",
            ],
        );
        let ask = r"rpm -qf --queryformat %{NAME}\n";
        machine.replies(
            &format!(
                "{ask} /usr/share/applications/wine.desktop /usr/share/applications/gimp.desktop /usr/share/applications/org.inkscape.Inkscape.desktop"
            ),
            true,
            "wine-desktop\nwine-desktop\ngimp\ninkscape\n",
            "",
        );
        for (file, owner) in [
            ("wine.desktop", "wine-desktop\nwine-desktop\n"),
            ("gimp.desktop", "gimp\n"),
            ("org.inkscape.Inkscape.desktop", "inkscape\n"),
        ] {
            machine.replies(
                &format!("{ask} /usr/share/applications/{file}"),
                true,
                owner,
                "",
            );
        }
        let found = owners(&machine, &asked, &folder);
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(found["wine-desktop"], folder.join("wine.desktop"));
        assert_eq!(found["gimp"], folder.join("gimp.desktop"));
        assert_eq!(
            found["inkscape"],
            folder.join("org.inkscape.Inkscape.desktop")
        );
        assert_eq!(
            machine.ran().len(),
            4,
            "the batch, then one call per launcher"
        );

        // One line per launcher (an unowned one says so with spaces): the
        // batch's answer is used as it is.
        let machine = Machine::new(&["rpm"]);
        let (asked, folder) = launchers(&machine, &["vmware.desktop", "gimp.desktop"]);
        machine.replies(
            &format!(
                "{ask} /usr/share/applications/vmware.desktop /usr/share/applications/gimp.desktop"
            ),
            false,
            "file /usr/share/applications/vmware.desktop is not owned by any package\ngimp\n",
            "",
        );
        let found = owners(&machine, &asked, &folder);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found["gimp"], folder.join("gimp.desktop"));
        assert_eq!(machine.ran().len(), 1);
    }
}
