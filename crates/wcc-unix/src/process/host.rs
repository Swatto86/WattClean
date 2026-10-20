//! The environment a program WattClean starts gets: the system's own, not
//! the one an AppImage's launcher made for WattClean. The launcher
//! (AppImageKit's `AppRun` and linuxdeploy's GTK hook) puts the AppImage's
//! own folders first in `LD_LIBRARY_PATH`, `PATH`, `XDG_DATA_DIRS`,
//! `PYTHONHOME`, GTK's and GIO's module paths and the like, sets
//! `GTK_THEME`, and moves into the AppImage. A system program started that
//! way loads libraries built for another distribution: on Arch Linux
//! pacman then cannot start (the AppImage's OpenSSL is older than the one
//! it needs), and a file manager opened to show a leftover reads the
//! AppImage's settings schemas instead of its own. So inside an AppImage
//! each such variable loses the AppImage's entries (and goes when nothing
//! is left), the launcher's own markers go, and the program starts in the
//! home folder. Elsewhere nothing changes.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// What the launcher sets without naming the AppImage's folder: its own
/// markers, and values it imposes on everything it starts (the system's
/// own `GTK_THEME`, if there was one, is already lost).
const LAUNCHER_ONLY: &[&str] = &[
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "PYTHONDONTWRITEBYTECODE",
    "GTK_THEME",
];

/// A variable to set (`Some`) or remove (`None`) for a started program.
type Change = (OsString, Option<OsString>);

/// How a started program's environment differs from this process's.
#[derive(Debug, Default)]
struct Host {
    changes: Vec<Change>,
    /// Where a started program begins, when this process's own working
    /// folder is inside the AppImage.
    start_in: Option<PathBuf>,
}

fn host() -> &'static Host {
    static HOST: OnceLock<Host> = OnceLock::new();
    HOST.get_or_init(|| {
        let Some(folders) = appimage_folders() else {
            return Host::default();
        };
        let folders: Vec<&Path> = folders.iter().map(PathBuf::as_path).collect();
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute() && home.is_dir());
        Host {
            changes: changes(std::env::vars_os(), &folders),
            start_in: Some(home.unwrap_or_else(|| PathBuf::from("/"))),
        }
    })
}

/// A command for `program` that runs with the system's own environment.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    apply(&mut command, host());
    command
}

/// A variable as the system set it, before an AppImage's launcher changed
/// it: `PATH` for looking a program up as the system would.
pub(super) fn var(name: &str) -> Option<OsString> {
    match host().changes.iter().find(|(changed, _)| changed == name) {
        Some((_, value)) => value.clone(),
        None => std::env::var_os(name),
    }
}

fn apply(command: &mut Command, host: &Host) {
    for (name, value) in &host.changes {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    if let Some(folder) = &host.start_in {
        command.current_dir(folder);
    }
}

/// The AppImage's folder (`APPDIR`), as given and as its real path, when
/// this program runs from inside it; `None` elsewhere, even when `APPDIR`
/// happens to be set.
fn appimage_folders() -> Option<Vec<PathBuf>> {
    let given = PathBuf::from(std::env::var_os("APPDIR")?);
    let real = std::fs::canonicalize(&given).ok()?;
    let exe = std::env::current_exe().ok()?;
    is_appdir(&real, &exe).then(|| vec![given, real])
}

/// Whether `exe` (a real path) runs from inside the AppImage folder
/// `appdir`: a whole folder, never `/` itself.
fn is_appdir(appdir: &Path, exe: &Path) -> bool {
    appdir.is_absolute() && appdir.parent().is_some() && exe.starts_with(appdir)
}

/// The changes that give a started program the system's environment back,
/// from this process's variables, for an AppImage in `folders` (one folder
/// in its spellings). A list's entries inside the AppImage go, and with
/// them empty entries (the current folder, to most programs); a variable
/// left with nothing is removed. A variable with no entry inside the
/// AppImage is left as it is.
fn changes(vars: impl IntoIterator<Item = (OsString, OsString)>, folders: &[&Path]) -> Vec<Change> {
    let inside = |entry: &[u8]| {
        folders
            .iter()
            .any(|folder| Path::new(OsStr::from_bytes(entry)).starts_with(folder))
    };
    vars.into_iter()
        .filter_map(|(name, value)| {
            if LAUNCHER_ONLY.iter().any(|own| name == *own) {
                return Some((name, None));
            }
            let entries: Vec<&[u8]> = value.as_bytes().split(|&byte| byte == b':').collect();
            if !entries.iter().any(|entry| inside(entry)) {
                return None;
            }
            let kept: Vec<&[u8]> = entries
                .into_iter()
                .filter(|entry| !entry.is_empty() && !inside(entry))
                .collect();
            let value = (!kept.is_empty()).then(|| OsString::from_vec(kept.join(&b':')));
            Some((name, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    fn change(name: &str, value: Option<&str>) -> Change {
        (OsString::from(name), value.map(OsString::from))
    }

    /// The variables WattClean 0.1.3's AppImage started with on Arch Linux
    /// (where pacman then failed on the AppImage's OpenSSL), with a session
    /// that already set `LD_LIBRARY_PATH` and `XDG_DATA_DIRS`.
    #[test]
    fn a_started_program_gets_the_systems_own_variables_back() {
        let app = "/tmp/.mount_WattClAbC123";
        let launched = vars(&[
            ("APPDIR", app),
            ("APPIMAGE", "/home/me/Apps/WattClean_0.1.3_amd64.AppImage"),
            ("ARGV0", "./WattClean_0.1.3_amd64.AppImage"),
            ("OWD", "/home/me"),
            ("HOME", "/home/me"),
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("GTK_THEME", "Adwaita:dark"),
            ("PYTHONDONTWRITEBYTECODE", "1"),
            (
                "LD_LIBRARY_PATH",
                "/tmp/.mount_WattClAbC123/usr/lib/:/tmp/.mount_WattClAbC123/usr/lib/x86_64-linux-gnu/:/tmp/.mount_WattClAbC123/lib64/:/opt/cuda/lib64",
            ),
            (
                "PATH",
                "/tmp/.mount_WattClAbC123/usr/bin/:/tmp/.mount_WattClAbC123/usr/sbin/:/usr/local/bin:/usr/bin",
            ),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_WattClAbC123/usr/share/:/tmp/.mount_WattClAbC123/usr/share:/usr/share:/home/me/.local/share/flatpak/exports/share:/var/lib/flatpak/exports/share",
            ),
            (
                "GSETTINGS_SCHEMA_DIR",
                "/tmp/.mount_WattClAbC123/usr/share/glib-2.0/schemas/:/tmp/.mount_WattClAbC123//usr/share/glib-2.0/schemas",
            ),
            (
                "GIO_MODULE_DIR",
                "/tmp/.mount_WattClAbC123//usr/lib/gio/modules",
            ),
            ("PYTHONHOME", "/tmp/.mount_WattClAbC123/usr/"),
            (
                "PERLLIB",
                "/tmp/.mount_WattClAbC123/usr/share/perl5/:/tmp/.mount_WattClAbC123/usr/lib/perl5/:",
            ),
            // Another folder whose name merely starts the same is not the AppImage's.
            ("MY_TOOLS", "/tmp/.mount_WattClAbC123x/bin:/opt/tools"),
            ("NOTE", "files in /tmp/.mount_WattClAbC123 vanish"),
        ]);
        let changed = changes(launched, &[Path::new(app)]);
        assert_eq!(
            changed,
            [
                change("APPDIR", None),
                change("APPIMAGE", None),
                change("ARGV0", None),
                change("OWD", None),
                change("GTK_THEME", None),
                change("PYTHONDONTWRITEBYTECODE", None),
                change("LD_LIBRARY_PATH", Some("/opt/cuda/lib64")),
                change("PATH", Some("/usr/local/bin:/usr/bin")),
                change(
                    "XDG_DATA_DIRS",
                    Some(
                        "/usr/share:/home/me/.local/share/flatpak/exports/share:/var/lib/flatpak/exports/share"
                    )
                ),
                change("GSETTINGS_SCHEMA_DIR", None),
                change("GIO_MODULE_DIR", None),
                change("PYTHONHOME", None),
                change("PERLLIB", None),
            ]
        );
    }

    /// The AppImage is recognised by where this program runs from, not by
    /// `APPDIR` alone, which any shell may have set.
    #[test]
    fn only_a_program_inside_the_appimage_is_in_one() {
        let exe = Path::new("/tmp/.mount_WattClAbC123/usr/bin/wattclean");
        assert!(is_appdir(Path::new("/tmp/.mount_WattClAbC123"), exe));
        assert!(is_appdir(Path::new("/tmp/.mount_WattClAbC123/"), exe));
        assert!(!is_appdir(Path::new("/tmp/.mount_WattCl"), exe));
        assert!(!is_appdir(Path::new("/"), exe));
        assert!(!is_appdir(Path::new("relative"), Path::new("relative/x")));
        assert!(!is_appdir(
            Path::new("/opt/elsewhere"),
            Path::new("/usr/bin/wattclean")
        ));
    }

    #[test]
    fn a_started_program_begins_outside_the_appimage_with_its_changes() {
        let host = Host {
            changes: vec![
                change("LD_LIBRARY_PATH", None),
                change("PATH", Some("/usr/bin")),
            ],
            start_in: Some(PathBuf::from("/home/me")),
        };
        let mut command = Command::new("true");
        apply(&mut command, &host);
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert!(envs.contains(&(OsStr::new("LD_LIBRARY_PATH"), None)));
        assert!(envs.contains(&(OsStr::new("PATH"), Some(OsStr::new("/usr/bin")))));
        assert_eq!(command.get_current_dir(), Some(Path::new("/home/me")));

        // Outside an AppImage a command is left as the standard library makes it.
        let mut plain = Command::new("true");
        apply(&mut plain, &Host::default());
        assert_eq!(plain.get_envs().count(), 0);
        assert_eq!(plain.get_current_dir(), None);
    }

    /// The test runner is no AppImage, so its programs start unchanged.
    #[test]
    fn outside_an_appimage_nothing_changes() {
        assert!(host().changes.is_empty());
        assert_eq!(host().start_in, None);
        assert_eq!(var("PATH"), std::env::var_os("PATH"));
        assert_eq!(command("true").get_envs().count(), 0);
    }
}
