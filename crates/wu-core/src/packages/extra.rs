//! Leftovers found other than by name: what a cask's own zap list says it
//! leaves (vendors' sub-folders such as `Application Support/Google/Chrome`
//! that names cannot reach), less what another installed program may own,
//! and the folder an app came in when no app is left in it (a game's data
//! beside its launcher).

use std::path::{Component, Path, PathBuf};

use super::keys::keys_for;
use super::leftovers::{Leftover, leftover};
use super::{Manager, Package, PackageSystem};

/// `their_keys` are the names every other installed package goes by and
/// `their_ids` their bundle identifiers, in lower case.
pub(super) fn leftovers(
    system: &dyn PackageSystem,
    home: &Path,
    removed: &Package,
    others: &[&Package],
    their_keys: &[String],
    their_ids: &[String],
) -> Vec<Leftover> {
    let mut paths = Vec::new();
    if removed.manager == Manager::HomebrewCask {
        let library = home.join("Library");
        for entry in &removed.zap {
            // A path another installed cask also trashes is shared (Google's
            // updater, a vendor's common folder): it stays.
            if others.iter().any(|other| other.zap.contains(entry)) {
                continue;
            }
            paths.extend(
                zap_paths(home, entry)
                    .into_iter()
                    .filter(|path| !anothers(&library, path, removed, their_keys, their_ids)),
            );
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

/// Whether a zap path may be another installed program's: named after a
/// name or bundle identifier it goes by (the WoW launcher's
/// `Caches/net.battle.bootstrapper` in Battle.net's list), or a vendor's
/// entry straight under a Library folder (`Application Support/Microsoft`,
/// `UBF8T346G9.Office`) that is not the app's own while another app of
/// that vendor (`com.microsoft`) stays. The longer identifier owns what
/// both prefix (Canary's `com.google.Chrome.canary` is not Chrome's).
fn anothers(
    library: &Path,
    path: &Path,
    removed: &Package,
    their_keys: &[String],
    their_ids: &[String],
) -> bool {
    let Some(file) = path.file_name() else {
        return true;
    };
    let file = file.to_string_lossy().to_lowercase();
    let name = [".plist", ".savedstate", ".binarycookies"]
        .iter()
        .find_map(|ext| file.strip_suffix(ext))
        .unwrap_or(&file);
    let own_id = removed.bundle_id.as_deref().map(str::to_lowercase);
    let own_id = own_id.as_deref();
    let by_id = their_ids.iter().any(|id| {
        of_bundle(name, id)
            && !own_id.is_some_and(|own| own.len() > id.len() && of_bundle(name, own))
    });
    if by_id || their_keys.iter().any(|key| key == name) {
        return true;
    }
    let vendor_level = path
        .strip_prefix(library)
        .is_ok_and(|rest| rest.components().count() == 2);
    let shared_vendor = vendors(removed)
        .iter()
        .any(|mine| their_ids.iter().any(|id| vendor(id) == Some(mine)));
    let own = own_id.is_some_and(|own| of_bundle(name, own))
        || keys_for(removed).iter().any(|key| key == name);
    vendor_level && shared_vendor && !own
}

/// Whether `name` (lower case) is the entry of the app `id` names: the
/// identifier itself, with a suffix (`<id>.shipit`) or after a team or
/// group prefix (`<TeamID>.<id>`).
fn of_bundle(name: &str, id: &str) -> bool {
    name == id
        || name
            .strip_prefix(id)
            .is_some_and(|rest| rest.starts_with('.'))
        || name
            .strip_suffix(id)
            .is_some_and(|rest| rest.ends_with('.'))
}

/// The vendors the removed app comes from: its bundle identifier's, or,
/// once its app is gone and the identifier with it, those of the
/// identifiers its zap list names straight under a Library folder
/// (`Containers/com.microsoft.Word` in Word's).
fn vendors(removed: &Package) -> Vec<String> {
    if let Some(id) = &removed.bundle_id {
        return vendor(&id.to_lowercase())
            .map(str::to_string)
            .into_iter()
            .collect();
    }
    removed
        .zap
        .iter()
        .filter_map(|entry| {
            let (_, name) = entry.strip_prefix("~/Library/")?.split_once('/')?;
            let name = name.to_lowercase();
            if name.contains('/') || name.matches('.').count() < 2 {
                return None;
            }
            vendor(&name).map(str::to_string)
        })
        .collect()
}

/// A bundle identifier's vendor: its first two labels (`com.microsoft`).
fn vendor(id: &str) -> Option<&str> {
    let first = id.find('.')?;
    let end = id[first + 1..]
        .find('.')
        .map_or(id.len(), |at| first + 1 + at);
    Some(&id[..end])
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

/// How many folders down an app is looked for in the folder an app came in
/// (`World of Warcraft/_anniversary_/World of Warcraft Classic.app` is two).
const APP_DEPTH: usize = 4;

/// The folder an app sat in, one below an Applications folder, once no app
/// is left anywhere in it: `/Applications/World of Warcraft` after its
/// launcher and games went. A suite's folder stays while any of its apps
/// does; Utilities always.
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
    let claimed = others
        .iter()
        .any(|other| Path::new(&other.id).starts_with(folder));
    (!claimed && !holds_app(folder, APP_DEPTH)).then(|| folder.to_path_buf())
}

/// Whether an app bundle is in `folder` or the folders below it, `depth`
/// levels down. Links are not followed (moving the folder moves the link,
/// not what it points at) nor bundles opened, and a folder that cannot be
/// read counts as holding one.
fn holds_app(folder: &Path, depth: usize) -> bool {
    let Ok(mut entries) = std::fs::read_dir(folder) else {
        return true;
    };
    entries.any(|entry| {
        let Ok(entry) = entry else {
            return true;
        };
        let path = entry.path();
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
            || (depth > 1
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
                && holds_app(&path, depth - 1))
    })
}

#[cfg(test)]
#[path = "extra_tests.rs"]
mod tests;
