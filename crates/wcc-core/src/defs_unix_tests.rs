//! Linux and macOS definitions that once named the wrong folder or program,
//! or went without the warning they need.

use super::*;
use crate::model::{Action, Cleaner, CleanerOption, Detect};

pub(super) const UNIX: [Platform; 2] = [Platform::Linux, Platform::Macos];

pub(super) fn cleaner(platform: Platform, id: &str) -> Cleaner {
    built_in(platform)
        .0
        .into_iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("{platform:?} has no {id}"))
}

pub(super) fn option<'a>(cleaner: &'a Cleaner, id: &str) -> &'a CleanerOption {
    cleaner
        .options
        .iter()
        .find(|o| o.id == id)
        .unwrap_or_else(|| panic!("{}.{id} is missing", cleaner.id))
}

fn deleted(option: &CleanerOption) -> Vec<&str> {
    option
        .actions
        .iter()
        .filter_map(|action| match action {
            Action::Delete(spec) => Some(spec.path.as_str()),
            _ => None,
        })
        .collect()
}

fn detected(cleaner: &Cleaner) -> Vec<&str> {
    cleaner
        .detect
        .iter()
        .filter_map(|detect| match detect {
            Detect::Path(path) => Some(path.as_str()),
            _ => None,
        })
        .collect()
}

/// `uno_packages/cache` holds the extensions the person installed and
/// their registration, which LibreOffice cannot make again; `backup` holds
/// what it recovers after a crash.
#[test]
fn libreoffice_keeps_extensions_and_warns_before_backups_go() {
    for platform in UNIX {
        let office = cleaner(platform, "libreoffice");
        for option in &office.options {
            assert!(
                deleted(option).iter().all(|p| !p.contains("uno_packages")),
                "{platform:?}: libreoffice.{} deletes extensions",
                option.id
            );
        }
        let temp = option(&office, "cache");
        assert_eq!(temp.name, "Temporary files");
        assert!(!deleted(temp).is_empty());
        assert!(deleted(temp).iter().all(|p| p.ends_with("/user/temp")));
        let backups = option(&office, "backups");
        assert!(
            backups
                .warning
                .as_deref()
                .is_some_and(|w| w.contains("crash")),
            "{platform:?}: {:?}",
            backups.warning
        );
    }
}

/// uv keeps its cache in `~/Library/Caches/uv` only when that folder was
/// there already; otherwise it follows XDG on macOS too.
#[test]
fn uv_is_found_in_either_cache_folder() {
    for platform in UNIX {
        let uv = cleaner(platform, "uv");
        for path in ["%XDG_CACHE_HOME%/uv", "%CACHE%/uv"] {
            assert!(detected(&uv).contains(&path), "{platform:?}: {path}");
            assert!(
                deleted(option(&uv, "cache")).contains(&path),
                "{platform:?}: {path}"
            );
        }
    }
}

/// VLC keeps album art in its caches folder; Application Support holds its
/// settings.
#[test]
fn vlc_on_macos_cleans_the_art_in_its_caches_folder() {
    let vlc = cleaner(Platform::Macos, "vlc");
    assert_eq!(
        deleted(option(&vlc, "cache")),
        vec!["%CACHE%/org.videolan.vlc/art"]
    );
    assert!(detected(&vlc).contains(&"%CACHE%/org.videolan.vlc"));
    assert!(detected(&vlc).contains(&"%CONFIG%/org.videolan.vlc"));
}

/// Running names are compared with `ps -A -c`'s, in lower case: new Teams
/// runs as MSTeams, VSCodium's app as VSCodium (codium is its command), and
/// "claude" is Claude Code's command as well as the desktop app.
#[test]
fn macos_running_names_are_the_names_ps_shows() {
    let running = |id: &str| cleaner(Platform::Macos, id).running;
    let claude = running("claude");
    assert!(!claude.iter().any(|name| name == "claude"), "{claude:?}");
    assert!(claude.iter().any(|name| name == "claude helper"));
    assert!(running("teams").iter().any(|name| name == "msteams"));
    let codium = running("vscodium");
    assert!(codium.iter().any(|name| name == "vscodium"), "{codium:?}");
    assert!(codium.iter().any(|name| name == "codium"), "{codium:?}");
}

/// A full start-up disk makes macOS remove Time Machine's local snapshots
/// and downloaded iCloud copies.
#[test]
fn macos_free_space_wipe_warns_of_lost_local_snapshots() {
    let system = cleaner(Platform::Macos, "system");
    let warning = option(&system, "free_disk_space").warning.clone();
    assert!(
        warning
            .as_deref()
            .is_some_and(|w| w.contains("Time Machine") && w.contains("iCloud")),
        "{warning:?}"
    );
}
