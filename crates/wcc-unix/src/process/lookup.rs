//! Finding a program by its bare name. One that runs as the account is
//! looked for where package managers and developer tools install, then on
//! `PATH`. One that runs as administrator comes only from the system's own
//! folders, and only when it, every folder above it and every link on the
//! way are root's alone: Homebrew's folders, the home folder and
//! `PATH` are the account's to write, so anything the account runs could
//! have put a program there.

use std::fs::Metadata;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Apple Silicon's Homebrew, searched first as Homebrew's own `shellenv`
/// puts it: a Mac moved from an Intel one may still hold an older Homebrew
/// in `/usr/local`, which would otherwise hide it.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const HOMEBREW_FIRST: &[&str] = &["/opt/homebrew/bin", "/opt/homebrew/sbin"];
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
const HOMEBREW_FIRST: &[&str] = &[];

/// Where package managers and developer tools install themselves, searched
/// before `PATH` (a desktop app's `PATH` is often short, and on macOS lacks
/// Homebrew and MacPorts). `~` is the home folder. Only for programs that
/// run as the account.
const FOLDERS: &[&str] = &[
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
    "/opt/homebrew/bin",
    "/opt/local/bin",
    "/home/linuxbrew/.linuxbrew/bin",
    "~/.linuxbrew/bin",
    "/snap/bin",
    "/nix/var/nix/profiles/default/bin",
    "~/.nix-profile/bin",
    "/usr/local/go/bin",
    "/usr/lib/go/bin",
    "~/.local/bin",
    "~/.local/share/pnpm",
    "~/.bun/bin",
    "~/miniconda3/bin",
    "~/miniforge3/bin",
    "~/anaconda3/bin",
    "/opt/conda/bin",
    "/Applications/Docker.app/Contents/Resources/bin",
    // pnpm's own home on macOS, then OrbStack's, Docker Desktop's and
    // Rancher Desktop's command-line tools when not linked elsewhere.
    "~/Library/pnpm",
    "~/.orbstack/bin",
    "~/.docker/bin",
    "~/.rd/bin",
];

/// Programs a version manager puts in place of the system's, searched in
/// `VERSION_MANAGERS` first as in the account's terminal: the account's own
/// Ruby is the one whose gems are its own, not macOS's in `/usr/bin`. Only
/// these: those folders belong to the account.
const VERSIONED: &[&str] = &["gem"];

const VERSION_MANAGERS: &[&str] = &[
    "~/.rbenv/shims",
    "~/.asdf/shims",
    "~/.local/share/mise/shims",
    "~/.rvm/wrappers/default",
];

/// The only folders a program run as administrator is taken from: the
/// system's own, and MacPorts' on macOS (it installs as root). Never
/// Homebrew's (`/usr/local` on an Intel Mac), the home folder or `PATH`.
#[cfg(target_os = "macos")]
const ADMIN_FOLDERS: &[&str] = &[
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
    "/opt/local/sbin",
    "/opt/local/bin",
];
#[cfg(not(target_os = "macos"))]
const ADMIN_FOLDERS: &[&str] = &[
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
];

/// The full path of an executable program called `name`, for running as
/// the account; `None` when none is installed. `name` is a bare name:
/// anything with a separator is refused.
pub fn find(name: &str, home: Option<&Path>) -> Option<PathBuf> {
    let path = super::host::var("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    candidates(name, home, path)
        .into_iter()
        .find(|candidate| is_executable(candidate))
}

/// Where [`find`] looks for `name`, in order, with `PATH`'s folders last.
fn candidates(name: &str, home: Option<&Path>, path: Vec<PathBuf>) -> Vec<PathBuf> {
    if !is_bare(name) {
        return Vec::new();
    }
    let first = if VERSIONED.contains(&name) {
        VERSION_MANAGERS
    } else {
        &[]
    };
    first
        .iter()
        .chain(HOMEBREW_FIRST)
        .chain(
            FOLDERS
                .iter()
                .filter(|folder| !HOMEBREW_FIRST.contains(folder)),
        )
        .filter_map(|folder| match folder.strip_prefix("~/") {
            Some(rest) => home.map(|home| home.join(rest)),
            None => Some(PathBuf::from(folder)),
        })
        .chain(path.into_iter().filter(|dir| dir.is_absolute()))
        .map(|dir| dir.join(name))
        .collect()
}

/// The full path of `name` for running as administrator: from
/// `ADMIN_FOLDERS` only, and only a copy that [`trusted`] accepts, by its
/// real path. One that fails is passed over, so a copy planted earlier in
/// the search cannot hide the real one.
pub fn find_trusted(name: &str) -> Option<PathBuf> {
    find_trusted_in(name, ADMIN_FOLDERS.iter().map(Path::new))
}

fn find_trusted_in<'a>(name: &str, folders: impl IntoIterator<Item = &'a Path>) -> Option<PathBuf> {
    if !is_bare(name) {
        return None;
    }
    folders
        .into_iter()
        .map(|folder| folder.join(name))
        .filter(|candidate| is_executable(candidate))
        .find_map(|candidate| trusted(&candidate).ok())
}

/// The program's real path (links followed) when it, every folder above it
/// and every folder holding a link on the way to it belong to root and
/// neither their group nor other accounts may write them: then only an
/// administrator can have put it there, changed it or pointed a link at it.
pub(super) fn trusted(program: &Path) -> io::Result<PathBuf> {
    match resolve_as_root(program)? {
        Some(real) if std::fs::metadata(&real)?.is_file() => Ok(real),
        _ => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is not run with administrator rights: it, or a folder holding it, can be changed without them",
                program.display()
            ),
        )),
    }
}

/// Links a path may pass through before it is taken for a loop.
const MAX_LINKS: usize = 40;

/// `path` with its links followed one part at a time, as the system follows
/// them, every folder and the file reached checked with [`root_only`] on
/// the way; `None` at the first that fails. A link is only ever met inside
/// a folder already checked, so nobody but root can have made or changed
/// it.
fn resolve_as_root(path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a full path", path.display()),
        ));
    }
    let root = Path::new("/");
    if !root_only(&std::fs::metadata(root)?) {
        return Ok(None);
    }
    // Parts still to follow, the next one last; "/" starts again at the
    // root, as an absolute link does.
    let parts = |path: &Path| -> Vec<std::ffi::OsString> {
        path.components()
            .rev()
            .map(|part| part.as_os_str().to_owned())
            .collect()
    };
    let mut pending = parts(path);
    let mut at = root.to_path_buf();
    let mut links = 0;
    while let Some(part) = pending.pop() {
        if part == "/" {
            at = root.to_path_buf();
        } else if part == ".." {
            at.pop();
        } else if part != "." {
            let next = at.join(&part);
            let meta = std::fs::symlink_metadata(&next)?;
            if meta.file_type().is_symlink() {
                links += 1;
                if links > MAX_LINKS {
                    return Err(io::Error::other(format!(
                        "{} leads through too many links",
                        path.display()
                    )));
                }
                pending.extend(parts(&std::fs::read_link(&next)?));
            } else if root_only(&meta) {
                at = next;
            } else {
                return Ok(None);
            }
        }
    }
    Ok(Some(at))
}

fn root_only(meta: &Metadata) -> bool {
    meta.uid() == 0 && meta.mode() & 0o022 == 0
}

fn is_bare(name: &str) -> bool {
    !name.is_empty() && !name.contains('/')
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable(folder: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(folder).unwrap();
        let file = folder.join(name);
        std::fs::write(&file, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        file
    }

    #[test]
    fn programs_are_found_by_bare_name_only() {
        assert!(find("sh", None).is_some());
        assert!(find("/bin/sh", None).is_none());
        assert!(find("../sh", None).is_none());
        assert!(find("no-such-program-wattclean", None).is_none());
        assert!(find_trusted("/bin/sh").is_none());
        assert!(find_trusted("").is_none());
    }

    /// The account's own Ruby is the one whose gems it cleans, not the
    /// system's in `/usr/bin`; nothing else is taken from a version
    /// manager's folders, which the account (and any program it runs) can
    /// write to.
    #[test]
    fn only_the_account_rubys_gem_comes_from_a_version_manager() {
        let home = tempfile::tempdir().unwrap();
        let shims = home.path().join(".rbenv/shims");
        for name in ["gem", "sh"] {
            executable(&shims, name);
        }
        assert_eq!(find("gem", Some(home.path())), Some(shims.join("gem")));
        let sh = find("sh", Some(home.path()));
        assert!(sh.is_some() && sh != Some(shims.join("sh")), "{sh:?}");
        assert!(find_trusted("gem").is_none_or(|gem| !gem.starts_with(home.path())));
    }

    /// A program the account could have written (here, one in a temporary
    /// folder of its own) is never taken for one run as administrator, and
    /// the search goes on past it to the system's copy.
    #[test]
    fn a_program_run_as_administrator_comes_only_from_folders_only_root_can_change() {
        let planted = tempfile::tempdir().unwrap();
        let sh = executable(planted.path(), "sh");
        assert_eq!(
            trusted(&sh).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let system_sh = std::fs::canonicalize("/bin/sh").unwrap();
        assert_eq!(trusted(Path::new("/bin/sh")).unwrap(), system_sh);
        assert_eq!(
            find_trusted_in("sh", [planted.path(), Path::new("/bin")]),
            Some(system_sh)
        );
        assert_eq!(find_trusted_in("sh", [planted.path()]), None);
        assert!(find_trusted("sh").is_some());

        // A link there could be pointed at any of root's programs, which
        // would then run with arguments meant for another.
        let port = planted.path().join("port");
        std::os::unix::fs::symlink("/bin/sh", &port).unwrap();
        assert!(trusted(&port).is_err());
        assert_eq!(find_trusted_in("port", [planted.path()]), None);
        // Root's own links are followed: one with `..` on macOS, `/bin`
        // (or `sh` itself) elsewhere.
        let link = if cfg!(target_os = "macos") {
            "/sbin/mount_apfs"
        } else {
            "/bin/sh"
        };
        assert!(std::fs::symlink_metadata(link).unwrap().is_symlink());
        assert_eq!(
            trusted(Path::new(link)).unwrap(),
            std::fs::canonicalize(link).unwrap()
        );
        for folder in ADMIN_FOLDERS {
            assert!(
                !folder.starts_with('~') && !folder.starts_with("/opt/homebrew"),
                "{folder}"
            );
            if cfg!(target_os = "macos") {
                assert!(!folder.starts_with("/usr/local"), "{folder}");
            }
        }
    }

    /// On Apple Silicon, Homebrew's own `/opt/homebrew` comes before an
    /// Intel Homebrew left in `/usr/local`.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn apple_silicons_homebrew_comes_before_an_intel_one() {
        let order = candidates("brew", None, Vec::new());
        let at = |folder: &str| {
            order
                .iter()
                .position(|candidate| candidate == &Path::new(folder).join("brew"))
                .unwrap()
        };
        assert!(at("/opt/homebrew/bin") < at("/usr/local/bin"), "{order:?}");
        assert_eq!(at("/opt/homebrew/bin"), 0);
    }

    /// pnpm's and the Docker tools' macOS homes are searched for programs
    /// run as the account, after the system's folders and before `PATH`.
    #[test]
    fn the_macos_homes_of_pnpm_and_docker_are_searched() {
        let home = Path::new("/home/someone");
        let path = vec![PathBuf::from("/from/path")];
        let docker = candidates("docker", Some(home), path.clone());
        for folder in [".orbstack/bin", ".docker/bin", ".rd/bin"] {
            let at = docker
                .iter()
                .position(|candidate| candidate == &home.join(folder).join("docker"))
                .unwrap();
            assert!(
                at > docker
                    .iter()
                    .position(|c| c == Path::new("/usr/bin/docker"))
                    .unwrap()
            );
            assert!(at < docker.len() - 1, "{docker:?}");
        }
        let pnpm = candidates("pnpm", Some(home), path);
        assert!(pnpm.contains(&home.join("Library/pnpm/pnpm")));
        assert_eq!(pnpm.last(), Some(&PathBuf::from("/from/path/pnpm")));
    }
}
