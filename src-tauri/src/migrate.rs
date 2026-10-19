//! Taking over from the three apps WattClean replaces. At the first start of
//! each module (its folder is missing or empty, as the command line leaves
//! it when run first), the old app's data is copied into it, and what could
//! not be copied (a locked file) is tried again at each start until it is
//! in; the window lists what failed: WattCompClean's settings and cleaners, WattRegClean's settings,
//! backups, tweaks and watch list, WattUninstaller's settings, backups,
//! history and install logs. Copied, never moved, so the old app keeps
//! working until it is uninstalled; its program files are never touched.
//! Schedules are not copied: their Task Scheduler tasks start the old app,
//! so they are set up again in WattClean.

use std::fs;
use std::io;
use std::path::Path;

/// One old app: its folder name under Local AppData, the module folder it
/// becomes, and the entries worth bringing.
struct Source {
    app: &'static str,
    module: &'static str,
    entries: &'static [&'static str],
}

const SOURCES: &[Source] = &[
    Source {
        app: "WattCompClean",
        module: "disk",
        entries: &["settings.json", "history.json", "cleaners"],
    },
    Source {
        app: "WattRegClean",
        module: "registry",
        entries: &[
            "settings.json",
            "tuner.json",
            "watch.json",
            "watch-logs",
            "backups",
        ],
    },
    Source {
        app: "WattUninstaller",
        module: "uninstall",
        entries: &["settings.json", "history.json", "backups", "logs"],
    },
];

/// What was brought over, for the log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Adopted {
    pub apps: Vec<&'static str>,
    pub problems: Vec<String>,
}

/// Left in a module folder while some of the old app's data is still to
/// copy, one entry per line. Written before copying, so a start cut short
/// (a locked file, a crash) is finished at the next start; removed once
/// everything is in.
const PENDING: &str = "adopting.txt";

/// Copies each old app's data from `local` (Local AppData) into `root`
/// (WattClean's data folder) when that module's folder is missing or
/// empty, or still lists entries in [`PENDING`]. What is already in the
/// module folder is never overwritten.
pub fn adopt(local: &Path, root: &Path) -> Adopted {
    let mut adopted = Adopted::default();
    for source in SOURCES {
        let from = local.join(source.app);
        let to = root.join(source.module);
        if !from.is_dir() {
            continue;
        }
        let pending = to.join(PENDING);
        let entries: Vec<&str> = if has_entries(&to) {
            // No list: adopted before, or WattClean's own data.
            let Ok(text) = fs::read_to_string(&pending) else {
                continue;
            };
            let listed: Vec<&str> = text.lines().map(str::trim).collect();
            source
                .entries
                .iter()
                .copied()
                .filter(|entry| listed.contains(entry))
                .collect()
        } else {
            source.entries.to_vec()
        };
        if let Err(error) = fs::create_dir_all(&to).and_then(|()| list(&pending, &entries)) {
            adopted.problems.push(format!("{}: {error}", to.display()));
            continue;
        }
        let mut left = Vec::new();
        for entry in entries {
            let path = from.join(entry);
            if !path.exists() {
                continue;
            }
            if let Err(error) = copy(&path, &to.join(entry)) {
                adopted
                    .problems
                    .push(format!("{}: {error}", path.display()));
                left.push(entry);
            }
        }
        let recorded = if left.is_empty() {
            fs::remove_file(&pending)
        } else {
            list(&pending, &left)
        };
        if let Err(error) = recorded {
            adopted
                .problems
                .push(format!("{}: {error}", pending.display()));
        }
        adopted.apps.push(source.app);
    }
    adopted
}

fn list(pending: &Path, entries: &[&str]) -> io::Result<()> {
    fs::write(pending, entries.join("\n"))
}

/// Whether a folder exists and holds anything.
fn has_entries(folder: &Path) -> bool {
    fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_some())
}

/// Copies a file, or a folder with everything in it, keeping whatever is
/// already at `to`. Links are skipped, never followed. Every part is
/// tried; the first problem is returned.
fn copy(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() || fs::symlink_metadata(to).is_ok_and(|m| !m.is_dir()) {
        return Ok(());
    }
    if !meta.is_dir() {
        return fs::copy(from, to).map(|_| ());
    }
    fs::create_dir_all(to)?;
    let mut outcome = Ok(());
    for entry in fs::read_dir(from)? {
        let copied = entry.and_then(|entry| copy(&entry.path(), &to.join(entry.file_name())));
        if outcome.is_ok() {
            outcome = copied;
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn copies_each_old_apps_data_once_and_leaves_it_in_place() {
        let local = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let old = local.path();
        write(&old.join(r"WattCompClean\settings.json"), "{}");
        write(&old.join(r"WattCompClean\wattcompclean.exe"), "program");
        write(&old.join(r"WattRegClean\backups\1\backup.json"), "[]");
        write(&old.join(r"WattRegClean\jobs.json"), "[]");
        write(&old.join(r"WattUninstaller\history.json"), "[]");

        let adopted = adopt(old, root.path());
        assert_eq!(
            adopted.apps,
            vec!["WattCompClean", "WattRegClean", "WattUninstaller"]
        );
        assert!(adopted.problems.is_empty(), "{:?}", adopted.problems);
        let new = root.path();
        assert!(new.join(r"disk\settings.json").is_file());
        assert!(
            !new.join(r"disk\wattcompclean.exe").exists(),
            "no program files"
        );
        assert!(new.join(r"registry\backups\1\backup.json").is_file());
        assert!(!new.join(r"registry\jobs.json").exists(), "no schedules");
        assert!(new.join(r"uninstall\history.json").is_file());
        assert!(old.join(r"WattRegClean\backups\1\backup.json").is_file());

        // A module that has its folder is never overwritten.
        write(
            &old.join(r"WattCompClean\settings.json"),
            r#"{"shred":true}"#,
        );
        assert!(adopt(old, new).apps.is_empty());
        assert_eq!(
            fs::read_to_string(new.join(r"disk\settings.json")).unwrap(),
            "{}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_copy_cut_short_by_a_locked_file_is_finished_at_the_next_start() {
        use std::os::windows::fs::OpenOptionsExt;
        let local = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let old = local.path().join("WattRegClean");
        write(&old.join("settings.json"), "{}");
        write(&old.join(r"backups\1\backup.json"), "one");
        write(&old.join(r"backups\2\backup.json"), "two");
        write(&old.join("watch.json"), "[]");
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(old.join(r"backups\1\backup.json"))
            .unwrap();
        let first = adopt(local.path(), root.path());
        assert_eq!(first.problems.len(), 1, "{:?}", first.problems);
        let new = root.path().join("registry");
        assert!(new.join("settings.json").is_file());
        assert!(new.join("watch.json").is_file());
        assert!(new.join(r"backups\2\backup.json").is_file());
        assert!(!new.join(r"backups\1\backup.json").exists());

        // Meanwhile the person changes a setting in WattClean.
        write(&new.join("settings.json"), r#"{"changed":true}"#);
        drop(locked);
        let second = adopt(local.path(), root.path());
        assert!(second.problems.is_empty(), "{:?}", second.problems);
        assert_eq!(
            fs::read_to_string(new.join(r"backups\1\backup.json")).unwrap(),
            "one"
        );
        assert_eq!(
            fs::read_to_string(new.join("settings.json")).unwrap(),
            r#"{"changed":true}"#,
            "what is already there is kept"
        );
        assert!(adopt(local.path(), root.path()) == Adopted::default());
    }

    #[test]
    fn an_empty_module_folder_still_takes_the_old_data() {
        let local = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        write(&local.path().join(r"WattRegClean\settings.json"), "{}");
        fs::create_dir_all(root.path().join("registry")).unwrap();
        assert_eq!(adopt(local.path(), root.path()).apps, vec!["WattRegClean"]);
        assert!(root.path().join(r"registry\settings.json").is_file());
    }

    #[test]
    fn nothing_happens_without_the_old_apps() {
        let local = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        assert_eq!(adopt(local.path(), root.path()), Adopted::default());
        assert!(fs::read_dir(root.path()).unwrap().next().is_none());
    }
}
