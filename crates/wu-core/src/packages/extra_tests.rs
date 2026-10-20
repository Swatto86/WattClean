use super::*;
use crate::packages::testing::{Machine, offered};

#[test]
fn zap_entries_must_stay_inside_the_accounts_library() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let chrome = home.join("Library/Application Support/Google/Chrome");
    std::fs::create_dir_all(&chrome).unwrap();
    std::fs::create_dir_all(home.join("Library/Group Containers/ABC.com.openai.sky.CUAService"))
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
fn zap_paths_named_after_another_program_are_its() {
    let library = Path::new("/home/Library");
    let cask = |id: &str| {
        let mut cask = Package::new(Manager::HomebrewCask, "google-chrome", "Google Chrome");
        cask.bundle_id = Some(id.into());
        cask
    };
    let chrome = cask("com.google.Chrome");
    let keys = ["ffmpeg".to_string()];
    let ids = ["com.google.chrome.canary".to_string()];
    let theirs = |removed: &Package, rest: &str, ids: &[String]| {
        anothers(library, &library.join(rest), removed, &keys, ids)
    };
    for rest in [
        "Application Support/Google/ffmpeg",
        "Caches/com.google.Chrome.canary.ShipIt",
        "Saved Application State/com.google.Chrome.canary.savedState",
        "Group Containers/EQHXZ8M8AV.com.google.Chrome.canary",
        "Google/GoogleSoftwareUpdate",
    ] {
        assert!(theirs(&chrome, rest, &ids), "{rest}");
    }
    for rest in [
        "Caches/com.google.Chrome.ShipIt",
        "Preferences/com.google.Chrome.plist",
        "Group Containers/EQHXZ8M8AV.com.google.Chrome",
        "Application Support/Google/Chrome",
    ] {
        assert!(!theirs(&chrome, rest, &ids), "{rest}");
    }
    let unmatched = Package::new(Manager::HomebrewCask, "chromium", "Chromium");
    assert!(
        theirs(
            &unmatched,
            "Group Containers/EQHXZ8M8AV.com.google.Chrome.canary",
            &ids
        ),
        "a team prefix before another's identifier"
    );
    assert!(
        !theirs(&chrome, "Google/GoogleSoftwareUpdate", &[]),
        "the vendor's last app"
    );
    assert!(theirs(&chrome, "Preferences/ffmpeg.plist", &[]));
    // The longer identifier owns what both prefix.
    let canary = cask("com.google.Chrome.canary");
    let chromes = ["com.google.chrome".to_string()];
    assert!(!theirs(
        &canary,
        "Caches/com.google.Chrome.canary.ShipIt",
        &chromes
    ));
    assert!(theirs(&canary, "Caches/com.google.Chrome.ShipIt", &chromes));
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

#[test]
fn a_folder_with_an_app_anywhere_inside_is_never_offered() {
    let machine = Machine::macos();
    let home = machine.home().unwrap();
    let game = machine.at("/Applications/World of Warcraft");
    let launcher = Package::new(
        Manager::MacApp,
        &game
            .join("World of Warcraft Launcher.app")
            .to_string_lossy(),
        "World of Warcraft Launcher",
    );
    let classic = game.join("_anniversary_/World of Warcraft Classic.app/Contents");
    std::fs::create_dir_all(&classic).unwrap();
    assert_eq!(
        lone_app_folder(&machine, &home, &launcher, &[]),
        None,
        "the game two folders down, never listed"
    );
    std::fs::remove_dir_all(game.join("_anniversary_")).unwrap();
    std::fs::create_dir_all(game.join("Utils/Tools/Mac/Repair.app")).unwrap();
    assert_eq!(
        lone_app_folder(&machine, &home, &launcher, &[]),
        None,
        "four folders down"
    );
    std::fs::remove_dir_all(game.join("Utils")).unwrap();
    std::fs::create_dir_all(game.join("_classic_/Data")).unwrap();
    assert_eq!(lone_app_folder(&machine, &home, &launcher, &[]), Some(game));
}

#[cfg(unix)]
#[test]
fn links_are_not_followed_and_unreadable_folders_may_hide_an_app() {
    use std::os::unix::fs::PermissionsExt;
    let machine = Machine::macos();
    let home = machine.home().unwrap();
    let game = machine.at("/Applications/Game");
    std::fs::create_dir_all(&game).unwrap();
    let elsewhere = machine.at("/Elsewhere");
    std::fs::create_dir_all(elsewhere.join("Other.app")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, game.join("link")).unwrap();
    let launcher = Package::new(
        Manager::MacApp,
        &game.join("Launcher.app").to_string_lossy(),
        "Launcher",
    );
    assert_eq!(
        lone_app_folder(&machine, &home, &launcher, &[]),
        Some(game.clone()),
        "the link goes, not what it points at"
    );
    let locked = game.join("Locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // An administrator reads every folder: nothing is hidden then.
    let hidden = std::fs::read_dir(&locked).is_err();
    let found = lone_app_folder(&machine, &home, &launcher, &[]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    if hidden {
        assert_eq!(found, None, "an unreadable folder may hold an app");
    }
}

/// A Mac whose Library holds these folders.
fn library_with(folders: &[&str]) -> Machine {
    let machine = Machine::macos();
    let library = machine.home().unwrap().join("Library");
    for folder in folders {
        std::fs::create_dir_all(library.join(folder)).unwrap();
    }
    machine
}

fn app(path: &str, name: &str, id: &str) -> Package {
    let mut app = Package::new(Manager::MacApp, path, name);
    app.bundle_id = Some(id.into());
    app
}

fn cask_of(token: &str, name: &str, id: Option<&str>, zap: &[&str]) -> Package {
    let mut cask = Package::new(Manager::HomebrewCask, token, name);
    cask.bundle_id = id.map(Into::into);
    cask.zap = zap
        .iter()
        .map(|entry| format!("~/Library/{entry}"))
        .collect();
    cask
}

#[test]
fn a_zap_entry_named_after_another_apps_identifier_stays() {
    let machine = library_with(&[
        "Caches/net.battle.bootstrapper",
        "Application Support/Battle.net",
    ]);
    let mut battle = cask_of(
        "battle-net",
        "Blizzard Battle.net",
        None,
        &[
            "Caches/net.battle.bootstrapper",
            "Application Support/Battle.net",
        ],
    );
    battle.known_as = vec!["Battle.net".into()];
    // The WoW launcher is an app no cask installed.
    let launcher = app(
        "/Applications/World of Warcraft/World of Warcraft Launcher.app",
        "World of Warcraft Launcher",
        "net.battle.bootstrapper",
    );
    assert_eq!(
        offered(&machine, &battle, &[battle.clone(), launcher]),
        ["Library/Application Support/Battle.net"]
    );
}

#[test]
fn a_vendors_shared_folder_stays_while_another_of_its_apps_does() {
    let machine = library_with(&[
        "Application Support/Microsoft",
        "Containers/com.microsoft.Word",
        "Group Containers/UBF8T346G9.Office",
        "Preferences",
    ]);
    let library = machine.home().unwrap().join("Library");
    std::fs::write(library.join("Preferences/com.microsoft.Word.plist"), b"p").unwrap();
    let word = cask_of(
        "microsoft-word",
        "Microsoft Word",
        Some("com.microsoft.Word"),
        &[
            "Application Support/Microsoft",
            "Containers/com.microsoft.Word",
            "Preferences/com.microsoft.Word.plist",
            "Group Containers/UBF8T346G9.Office",
        ],
    );
    let own = [
        "Library/Containers/com.microsoft.Word",
        "Library/Preferences/com.microsoft.Word.plist",
    ];
    let excel = app(
        "/Applications/Microsoft Excel.app",
        "Microsoft Excel",
        "com.microsoft.Excel",
    );
    assert_eq!(offered(&machine, &word, &[word.clone(), excel]), own);
    let excel = cask_of(
        "microsoft-excel",
        "Microsoft Excel",
        Some("com.microsoft.Excel"),
        &[],
    );
    assert_eq!(
        offered(&machine, &word, &[word.clone(), excel.clone()]),
        own
    );
    assert_eq!(
        offered(&machine, &word, std::slice::from_ref(&word)).len(),
        4,
        "the last of the vendor's apps"
    );
    // Word's app is gone, and its identifier with it: its zap list names
    // the vendor.
    let mut orphan = word.clone();
    orphan.bundle_id = None;
    let found = offered(&machine, &orphan, &[orphan.clone(), excel]);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(
        offered(&machine, &orphan, std::slice::from_ref(&orphan)).len(),
        4
    );
}

#[test]
fn a_vendors_deeper_folder_is_the_apps_and_the_longest_identifier_owns() {
    let machine = library_with(&[
        "Application Support/Google/Chrome",
        "Google/GoogleSoftwareUpdate",
        "Caches/com.google.Chrome.canary",
        "Caches/com.google.Chrome.ShipIt",
    ]);
    let chrome = cask_of(
        "google-chrome",
        "Google Chrome",
        Some("com.google.Chrome"),
        &[
            "Application Support/Google/Chrome",
            "Google/GoogleSoftwareUpdate",
            "Caches/com.google.Chrome*",
        ],
    );
    let drive = app(
        "/Applications/Google Drive.app",
        "Google Drive",
        "com.google.drivefs",
    );
    let canary = app(
        "/Applications/Google Chrome Canary.app",
        "Google Chrome Canary",
        "com.google.Chrome.canary",
    );
    assert_eq!(
        offered(&machine, &chrome, &[chrome.clone(), drive, canary]),
        [
            "Library/Application Support/Google/Chrome",
            "Library/Caches/com.google.Chrome.ShipIt"
        ]
    );
    let canary = cask_of(
        "google-chrome@canary",
        "Google Chrome Canary",
        Some("com.google.Chrome.canary"),
        &["Caches/com.google.Chrome.canary"],
    );
    let kept = app(
        "/Applications/Google Chrome.app",
        "Google Chrome",
        "com.google.Chrome",
    );
    assert_eq!(
        offered(&machine, &canary, &[canary.clone(), kept]),
        ["Library/Caches/com.google.Chrome.canary"]
    );
}
