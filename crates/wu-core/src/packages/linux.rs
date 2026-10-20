//! What Linux's package managers installed: the system's own (dpkg, rpm or
//! pacman), Flatpak, Snap, Homebrew and AppImage files. A system package
//! is an application when it installed a launcher (`.desktop` file); the
//! launcher's name is the name people know.

use super::{Manager, Package, PackageSystem, launchers, macos, parse_size, query};

/// The system's own package managers, in the order they are asked, by the
/// program that lists what each installed.
const NATIVE: [(&str, Manager); 3] = [
    ("dpkg-query", Manager::Apt),
    ("rpm", Manager::Rpm),
    ("pacman", Manager::Pacman),
];

pub fn list(system: &dyn PackageSystem) -> (Vec<Package>, Vec<String>) {
    let (mut packages, native, mut problems) = native(system);
    let mut add = |found: Option<Vec<Package>>, what: &str| match found {
        Some(found) => packages.extend(found),
        None => problems.push(format!("{what} could not list its packages")),
    };
    if system.find_program("flatpak").is_some() {
        add(flatpak(system), "Flatpak");
    }
    if system.find_program("snap").is_some() {
        add(snap(system), "Snap");
    }
    if system.find_program("brew").is_some() {
        add(macos::homebrew(system), "Homebrew");
    }
    packages.extend(appimages(system));
    if let Some(native) = native {
        launchers::mark_apps(system, native, &mut packages);
    }
    (packages, problems)
}

/// What the system's own package manager installed, and which one it is:
/// the first that lists anything. Another may be installed beside it with
/// nothing to list (Arch Linux packages dpkg and rpm for converting other
/// systems' packages), so one that lists nothing, or fails, is passed
/// over; its failure is reported only when no other answers.
fn native(system: &dyn PackageSystem) -> (Vec<Package>, Option<Manager>, Vec<String>) {
    let mut problems = Vec::new();
    for (program, manager) in NATIVE {
        if system.find_program(program).is_none() {
            continue;
        }
        let found = match manager {
            Manager::Apt => dpkg(system),
            Manager::Rpm => rpm(system),
            Manager::Pacman => pacman(system),
            _ => continue,
        };
        match found {
            Some(found) if !found.is_empty() => return (found, Some(manager), Vec::new()),
            Some(_) => {}
            None => problems.push(format!("{} could not list its packages", label(program))),
        }
    }
    (Vec::new(), None, problems)
}

/// What a listing program is called in a message: dpkg for `dpkg-query`.
fn label(program: &str) -> &str {
    program.strip_suffix("-query").unwrap_or(program)
}

/// What dpkg prints for each package.
const DPKG_FORMAT: &str = r"--showformat=${db:Status-Abbrev}\t${Package}\t${Version}\t${Installed-Size}\t${binary:Summary}\n";

/// dpkg's installed packages; sizes are in KiB.
fn dpkg(system: &dyn PackageSystem) -> Option<Vec<Package>> {
    let text = query(system, "dpkg-query", &["-W", DPKG_FORMAT])?;
    Some(parse_dpkg(&text))
}

pub(crate) fn parse_dpkg(text: &str) -> Vec<Package> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [status, name, version, size, summary, ..] = fields.as_slice() else {
                return None;
            };
            if !status.starts_with("ii") || name.is_empty() {
                return None;
            }
            let mut package = Package::new(Manager::Apt, name, name);
            package.version = Some((*version).to_string());
            package.size = size.trim().parse::<u64>().ok().map(|kib| kib * 1024);
            package.summary = Some((*summary).to_string()).filter(|s| !s.is_empty());
            Some(package)
        })
        .collect()
}

fn rpm(system: &dyn PackageSystem) -> Option<Vec<Package>> {
    let format = r"%{NAME}\t%{VERSION}-%{RELEASE}\t%{SIZE}\t%{SUMMARY}\n";
    let text = query(system, "rpm", &["-qa", "--queryformat", format])?;
    Some(parse_rpm(&text))
}

pub(crate) fn parse_rpm(text: &str) -> Vec<Package> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [name, version, size, summary, ..] = fields.as_slice() else {
                return None;
            };
            // Signing keys are listed as packages but are not software.
            if name.is_empty() || *name == "gpg-pubkey" {
                return None;
            }
            let mut package = Package::new(Manager::Rpm, name, name);
            package.version = Some((*version).to_string());
            package.size = size.trim().parse().ok();
            package.summary = Some((*summary).to_string()).filter(|s| !s.is_empty());
            Some(package)
        })
        .collect()
}

fn pacman(system: &dyn PackageSystem) -> Option<Vec<Package>> {
    let text = query(system, "pacman", &["-Qi"])?;
    Some(parse_pacman(&text))
}

/// `pacman -Qi`: one block of `Key : value` lines per package.
pub(crate) fn parse_pacman(text: &str) -> Vec<Package> {
    text.split("\n\n")
        .filter_map(|block| {
            let field = |key: &str| {
                block.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    (name.trim() == key).then(|| value.trim().to_string())
                })
            };
            let name = field("Name")?;
            let mut package = Package::new(Manager::Pacman, &name, &name);
            package.version = field("Version");
            package.size = field("Installed Size").and_then(|size| parse_size(&size));
            package.summary = field("Description");
            Some(package)
        })
        .collect()
}

fn flatpak(system: &dyn PackageSystem) -> Option<Vec<Package>> {
    let text = query(
        system,
        "flatpak",
        &[
            "list",
            "--app",
            "--columns=application,name,version,size,installation",
        ],
    )?;
    Some(parse_flatpak(&text))
}

pub(crate) fn parse_flatpak(text: &str) -> Vec<Package> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
            let [id, name, version, size, installation, ..] = fields.as_slice() else {
                return None;
            };
            if id.is_empty() || *id == "Application ID" {
                return None;
            }
            let mut package = Package::new(
                Manager::Flatpak,
                id,
                if name.is_empty() { id } else { name },
            );
            package.version = Some((*version).to_string()).filter(|v| !v.is_empty());
            package.size = parse_size(size);
            package.app = true;
            package.installation = Some((*installation).to_string());
            Some(package)
        })
        .collect()
}

fn snap(system: &dyn PackageSystem) -> Option<Vec<Package>> {
    let text = query(system, "snap", &["list"])?;
    let mut packages = parse_snap(&text);
    // A snap is an application when snapd exported a launcher for it
    // (`<snap>_<app>.desktop`): runtimes and themes (gnome-42-2204,
    // gtk-common-themes, mesa-2404) export none.
    let launchers: Vec<String> =
        std::fs::read_dir(system.at("/var/lib/snapd/desktop/applications"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|name| name.ends_with(".desktop"))
                    .collect()
            })
            .unwrap_or_default();
    for package in &mut packages {
        package.app = launchers
            .iter()
            .any(|launcher| launcher.starts_with(&format!("{}_", package.id)));
        package.size = std::fs::read_dir(system.at("/var/lib/snapd/snaps"))
            .ok()
            .and_then(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        name.starts_with(&format!("{}_", package.id)) && name.ends_with(".snap")
                    })
                    .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
                    .max()
            });
    }
    Some(packages)
}

/// `snap list`: the system's own snaps (bases, snapd, kernels) are left
/// out; the rest are applications until their launchers say otherwise.
pub(crate) fn parse_snap(text: &str) -> Vec<Package> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let (name, version, notes) = (words.first()?, words.get(1)?, words.last()?);
            let system = notes
                .split(',')
                .any(|note| matches!(note, "base" | "core" | "snapd" | "gadget" | "kernel"));
            if system || name.is_empty() {
                return None;
            }
            let mut package = Package::new(Manager::Snap, name, name);
            package.version = Some((*version).to_string());
            package.app = true;
            Some(package)
        })
        .collect()
}

/// AppImage files where people keep them.
fn appimages(system: &dyn PackageSystem) -> Vec<Package> {
    let Some(home) = system.home() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for folder in ["Applications", "AppImages", ".local/bin", "bin"] {
        let Ok(entries) = std::fs::read_dir(home.join(folder)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_appimage = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"));
            if !is_appimage || !path.is_file() {
                continue;
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut package =
                Package::new(Manager::AppImage, &path.to_string_lossy(), &app_name(&stem));
            package.size = entry.metadata().ok().map(|meta| meta.len());
            package.app = true;
            out.push(package);
        }
    }
    out
}

/// `Krita-5.2.2-x86_64` → `Krita`: an AppImage's name without its version
/// and architecture.
fn app_name(stem: &str) -> String {
    let words: Vec<&str> = stem.split(['-', '_']).collect();
    let name: Vec<&str> = words
        .iter()
        .take_while(|word| {
            !word.starts_with(|c: char| c.is_ascii_digit())
                && !matches!(
                    **word,
                    "x86" | "amd64" | "aarch64" | "arm64" | "i386" | "i686"
                )
        })
        .copied()
        .collect();
    if name.is_empty() {
        stem.to_string()
    } else {
        name.join(" ")
    }
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;
