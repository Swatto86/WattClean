//! Leftovers found other than by name: what a cask's own zap list says it
//! leaves (vendors' sub-folders such as `Application Support/Google/Chrome`
//! that names cannot reach), and the folder an app came in when nothing
//! else is left in it (a game's data beside its launcher).

use std::path::{Component, Path, PathBuf};

use super::leftovers::{Leftover, leftover};
use super::{Manager, Package, PackageSystem};

pub(super) fn leftovers(
    system: &dyn PackageSystem,
    home: &Path,
    removed: &Package,
    others: &[&Package],
) -> Vec<Leftover> {
    let mut paths = Vec::new();
    if removed.manager == Manager::HomebrewCask {
        for entry in &removed.zap {
            // A path another installed cask also trashes is shared (Google's
            // updater, a vendor's common folder): it stays.
            if others.iter().any(|other| other.zap.contains(entry)) {
                continue;
            }
            paths.extend(zap_paths(home, entry));
        }
    }
    if removed.manager == Manager::MacApp {
        paths.extend(lone_app_folder(system, home, removed, others));
    }
    paths.iter().map(|path| leftover(path)).collect()
}

/// What one zap `trash` entry names in this home folder: only something
/// inside the account's own Library, at least two folders down, with `*`
/// only in the last name and that name mostly literal.
fn zap_paths(home: &Path, entry: &str) -> Vec<PathBuf> {
    let Some(rest) = entry.strip_prefix("~/Library/") else {
        return Vec::new();
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let safe = parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && *p != "." && *p != ".." && !p.contains('\\'))
        && parts[..parts.len() - 1].iter().all(|p| !p.contains('*'));
    let Some((last, folders)) = parts.split_last().filter(|_| safe) else {
        return Vec::new();
    };
    let mut parent = home.join("Library");
    for folder in folders {
        parent.push(folder);
    }
    if !last.contains('*') {
        let path = parent.join(last);
        return if std::fs::symlink_metadata(&path).is_ok() {
            vec![path]
        } else {
            Vec::new()
        };
    }
    if last.chars().filter(|c| *c != '*').count() < 6 {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&parent) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| wildcard(last, &e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// `*` matches any run of characters; case is ignored.
fn wildcard(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let name = name.to_lowercase();
    let pieces: Vec<&str> = pattern.split('*').collect();
    let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
        return false;
    };
    if !name.starts_with(first) || name.len() < first.len() + last.len() || !name.ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for piece in &pieces[1..pieces.len() - 1] {
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    true
}

/// The folder an app sat in, one below an Applications folder, once no app
/// is left in it: `/Applications/World of Warcraft` after its launcher went.
/// A suite's folder stays while any of its apps does; Utilities always.
fn lone_app_folder(
    system: &dyn PackageSystem,
    home: &Path,
    removed: &Package,
    others: &[&Package],
) -> Option<PathBuf> {
    let bundle = Path::new(&removed.id);
    let folder = bundle.parent()?;
    let roots = [system.at("/Applications"), home.join("Applications")];
    let within = folder
        .parent()
        .is_some_and(|up| roots.iter().any(|root| root == up));
    let real = std::fs::symlink_metadata(folder).is_ok_and(|meta| meta.is_dir());
    let named = folder
        .file_name()
        .is_some_and(|name| !name.eq_ignore_ascii_case("Utilities"));
    if !(within && real && named) || folder.components().any(|c| c == Component::ParentDir) {
        return None;
    }
    let holds_app = std::fs::read_dir(folder).ok()?.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
    });
    let claimed = others
        .iter()
        .any(|other| Path::new(&other.id).starts_with(folder));
    (!holds_app && !claimed).then(|| folder.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::testing::Machine;

    #[test]
    fn zap_entries_must_stay_inside_the_accounts_library() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let chrome = home.join("Library/Application Support/Google/Chrome");
        std::fs::create_dir_all(&chrome).unwrap();
        std::fs::create_dir_all(
            home.join("Library/Group Containers/ABC.com.openai.sky.CUAService"),
        )
        .unwrap();
        assert_eq!(
            zap_paths(home, "~/Library/Application Support/Google/Chrome"),
            [chrome]
        );
        assert_eq!(
            zap_paths(
                home,
                "~/Library/Group Containers/*.com.openai.sky.CUAService"
            )
            .len(),
            1
        );
        for refused in [
            "~/Library/Application Support",
            "~/Library/../Documents/x",
            "~/Library/*/Chrome",
            "~/Library/Caches/*",
            "/Library/Application Support/Google",
            "~/Documents/notes",
        ] {
            assert!(zap_paths(home, refused).is_empty(), "{refused}");
        }
    }

    #[test]
    fn wildcards_match_whole_names() {
        assert!(wildcard(
            "*.com.openai.sky.CUAService",
            "2DC432GLL2.com.openai.sky.cuaservice"
        ));
        assert!(wildcard("com.google.Chrome*", "com.google.Chrome.helper"));
        assert!(!wildcard("*.com.openai.sky", "com.openai.sky.other"));
    }

    #[test]
    fn a_lone_apps_folder_is_offered_once_nothing_else_is_in_it() {
        let machine = Machine::macos();
        let apps = machine.at("/Applications");
        let game = apps.join("World of Warcraft");
        std::fs::create_dir_all(game.join("_retail_/Data")).unwrap();
        let suite = apps.join("Suite");
        std::fs::create_dir_all(suite.join("Other.app")).unwrap();
        std::fs::create_dir_all(apps.join("Utilities")).unwrap();
        let home = machine.home().unwrap();
        let removed = |path: &Path| Package::new(Manager::MacApp, &path.to_string_lossy(), "x");
        let launcher = removed(&game.join("World of Warcraft Launcher.app"));
        assert_eq!(
            lone_app_folder(&machine, &home, &launcher, &[]),
            Some(game.clone())
        );
        let in_suite = removed(&suite.join("One.app"));
        assert_eq!(
            lone_app_folder(&machine, &home, &in_suite, &[]),
            None,
            "Other.app is still there"
        );
        let utility = removed(&apps.join("Utilities/Tool.app"));
        assert_eq!(lone_app_folder(&machine, &home, &utility, &[]), None);
        let top = removed(&apps.join("Slack.app"));
        assert_eq!(lone_app_folder(&machine, &home, &top, &[]), None);
        let sibling = removed(&game.join("_retail_/World of Warcraft.app"));
        assert_eq!(
            lone_app_folder(&machine, &home, &launcher, &[&sibling]),
            None,
            "a listed app still lives there"
        );
    }
}
