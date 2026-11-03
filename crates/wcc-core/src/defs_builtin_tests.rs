use super::*;
use crate::model::{Action, Cleaner, CleanerOption};

const PLATFORMS: [Platform; 3] = [Platform::Windows, Platform::Linux, Platform::Macos];

fn option_of<'a>(cleaners: &'a [Cleaner], cleaner: &str, option: &str) -> &'a CleanerOption {
    cleaners
        .iter()
        .find(|c| c.id == cleaner)
        .and_then(|c| c.options.iter().find(|o| o.id == option))
        .unwrap_or_else(|| panic!("{cleaner}.{option} is missing"))
}

#[test]
fn profile_wide_deep_scans_stay_out_of_onedrive() {
    let (cleaners, _) = built_in(Platform::Windows);
    let scan = cleaners.iter().find(|c| c.id == "deep_scan").unwrap();
    for option in &scan.options {
        for action in &option.actions {
            let Action::Delete(spec) = action else {
                continue;
            };
            if spec.path == "%USERPROFILE%" {
                assert!(
                    spec.exclude.iter().any(|e| e == "%ONEDRIVE%"),
                    "deep_scan.{} walks into OneDrive",
                    option.id
                );
            }
        }
    }
}

#[test]
fn options_that_lose_pinned_or_unsynced_things_say_so() {
    let (cleaners, _) = built_in(Platform::Windows);
    let warns = |c, o, word: &str| {
        option_of(&cleaners, c, o)
            .warning
            .as_deref()
            .is_some_and(|w| w.to_lowercase().contains(word))
    };
    assert!(warns("explorer", "recent", "pinned"));
    assert!(warns("office", "cache", "upload"));
}

#[test]
fn site_storage_leaves_browser_extension_data_alone() {
    let mut checked = 0;
    let all: Vec<Cleaner> = PLATFORMS
        .iter()
        .flat_map(|platform| built_in(*platform).0)
        .collect();
    for cleaner in &all {
        for option in cleaner.options.iter().filter(|o| o.id == "site_storage") {
            for action in &option.actions {
                let Action::Delete(spec) = action else {
                    continue;
                };
                let marker = if spec.path.ends_with(r"\storage\default") {
                    "moz-extension+++"
                } else if spec.path.ends_with(r"\IndexedDB") {
                    "chrome-extension_"
                } else {
                    continue;
                };
                checked += 1;
                assert!(
                    spec.exclude.iter().any(|e| e.contains(marker)),
                    "{}.{} deletes extension data",
                    cleaner.id,
                    option.id
                );
            }
        }
    }
    assert!(checked >= 2, "only {checked} storage actions found");
}
