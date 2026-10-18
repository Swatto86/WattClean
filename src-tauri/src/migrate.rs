//! Taking over from the three apps WattClean replaces. At the first start of
//! each module (its folder is missing or empty, as the command line leaves
//! it when run first), the old app's data is copied into
//! it: WattCompClean's settings and cleaners, WattRegClean's settings,
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

/// Copies each old app's data from `local` (Local AppData) into `root`
/// (WattClean's data folder) when that module's folder is missing or empty.
pub fn adopt(local: &Path, root: &Path) -> Adopted {
    let mut adopted = Adopted::default();
    for source in SOURCES {
        let from = local.join(source.app);
        let to = root.join(source.module);
        if !from.is_dir() || has_entries(&to) {
            continue;
        }
        if let Err(error) = fs::create_dir_all(&to) {
            adopted.problems.push(format!("{}: {error}", to.display()));
            continue;
        }
        for entry in source.entries {
            let path = from.join(entry);
            if !path.exists() {
                continue;
            }
            if let Err(error) = copy(&path, &to.join(entry)) {
                adopted
                    .problems
                    .push(format!("{}: {error}", path.display()));
            }
        }
        adopted.apps.push(source.app);
    }
    adopted
}

/// Whether a folder exists and holds anything.
fn has_entries(folder: &Path) -> bool {
    fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_some())
}

/// Copies a file, or a folder with everything in it. Links are skipped,
/// never followed.
fn copy(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ())
    }
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
