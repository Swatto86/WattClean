use super::*;
use crate::packages::testing::Machine;

#[test]
fn keys_come_from_the_id_the_name_and_the_bundle() {
    let mut chrome = Package::new(Manager::Apt, "google-chrome-stable", "Google Chrome");
    chrome.app = true;
    let keys = keys_for(&chrome);
    assert!(keys.contains(&"google-chrome".to_string()), "{keys:?}");
    assert!(keys.contains(&"googlechrome".to_string()));
    let flatpak = Package::new(
        Manager::Flatpak,
        "org.gimp.GIMP",
        "GNU Image Manipulation Program",
    );
    assert!(keys_for(&flatpak).contains(&"org.gimp.gimp".to_string()));
    let generic = Package::new(Manager::Apt, "fonts", "Fonts");
    assert!(keys_for(&generic).is_empty());
}

#[test]
fn leftovers_are_found_by_exact_name_and_never_another_programs() {
    let machine = Machine::new(&[]);
    let home = machine.home().unwrap();
    for folder in [
        ".config/vlc",
        ".cache/vlc",
        ".local/share/vlc",
        ".vlc",
        ".config/vlc-extras",
        ".config/mpv",
        ".config/firefox",
        ".mozilla",
        ".var/app/org.videolan.VLC",
    ] {
        std::fs::create_dir_all(home.join(folder)).unwrap();
    }
    std::fs::write(home.join(".config/vlc/vlcrc"), b"settings").unwrap();
    std::fs::create_dir_all(home.join(".local/share/applications")).unwrap();
    std::fs::write(home.join(".local/share/applications/vlc.desktop"), b"").unwrap();

    let vlc = Package::new(Manager::Apt, "vlc", "VLC media player");
    let installed = vec![
        vlc.clone(),
        Package::new(Manager::Snap, "firefox", "firefox"),
    ];
    let found = find_leftovers(&machine, &vlc, &installed);
    let names: Vec<String> = found
        .iter()
        .map(|l| {
            l.path
                .strip_prefix(&home)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(
        names,
        [
            ".cache/vlc",
            ".config/vlc",
            ".local/share/applications/vlc.desktop",
            ".local/share/vlc",
            ".vlc"
        ]
    );
    assert!(found.iter().any(|l| l.size == Some(8) && l.folder));

    // Firefox from APT goes while the snap stays: its folders are the snap's too.
    let deb = Package::new(Manager::Apt, "firefox", "Firefox");
    let mut both = installed.clone();
    both.push(deb.clone());
    assert!(find_leftovers(&machine, &deb, &both).is_empty());
}

#[test]
fn a_mac_app_or_cask_never_offers_a_home_dot_folder() {
    let machine = Machine::macos();
    let home = machine.home().unwrap();
    for folder in [".claude", ".vim", "Library/Application Support/Claude"] {
        std::fs::create_dir_all(home.join(folder)).unwrap();
    }
    for manager in [Manager::HomebrewCask, Manager::MacApp] {
        let claude = Package::new(manager, "claude", "Claude");
        let found = find_leftovers(&machine, &claude, std::slice::from_ref(&claude));
        assert!(
            !found.iter().any(|l| l.path.ends_with(".claude")),
            "{found:#?}"
        );
        assert!(
            found
                .iter()
                .any(|l| l.path.ends_with("Application Support/Claude"))
        );
    }
    let vim = Package::new(Manager::Homebrew, "vim", "vim");
    let found = find_leftovers(&machine, &vim, std::slice::from_ref(&vim));
    assert!(
        found.iter().any(|l| l.path.ends_with(".vim")),
        "a formula's own dot-folder"
    );
}

#[test]
fn a_tools_folders_stay_while_a_program_of_its_name_does() {
    let machine = Machine::macos();
    let home = machine.home().unwrap();
    for folder in [".config/gh", ".cache/gh", ".local/share/gh", ".config/git"] {
        std::fs::create_dir_all(home.join(folder)).unwrap();
    }
    let gh = Package::new(Manager::Homebrew, "gh", "gh");
    assert_eq!(
        find_leftovers(&machine, &gh, std::slice::from_ref(&gh)).len(),
        3
    );
    // Apple's own git is still there after Homebrew's goes.
    machine.prints("git", "");
    let git = Package::new(Manager::Homebrew, "git", "git");
    assert!(find_leftovers(&machine, &git, std::slice::from_ref(&git)).is_empty());
}

#[test]
fn suffixed_bundle_entries_are_the_apps_unless_a_longer_id_owns_them() {
    let machine = Machine::macos();
    let library = machine.home().unwrap().join("Library");
    for folder in [
        "Caches/com.google.chrome.shipit",
        "Caches/com.google.chrome.canary.shipit",
        "Containers/com.google.chrome.helper",
        "Application Scripts/ABCDE12345.com.google.chrome",
        "Caches/com.google.chromecast",
    ] {
        std::fs::create_dir_all(library.join(folder)).unwrap();
    }
    let mut chrome = Package::new(
        Manager::MacApp,
        "/Applications/Google Chrome.app",
        "Google Chrome",
    );
    chrome.bundle_id = Some("com.google.Chrome".into());
    let mut canary = Package::new(
        Manager::MacApp,
        "/Applications/Canary.app",
        "Google Chrome Canary",
    );
    canary.bundle_id = Some("com.google.Chrome.canary".into());
    let found = find_leftovers(&machine, &chrome, &[chrome.clone(), canary]);
    let names: Vec<String> = found
        .iter()
        .map(|l| l.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "ABCDE12345.com.google.chrome",
            "com.google.chrome.shipit",
            "com.google.chrome.helper"
        ],
        "{names:?}"
    );
}

#[test]
fn a_casks_zap_list_finds_what_names_cannot() {
    let machine = Machine::macos();
    let home = machine.home().unwrap();
    let profile = home.join("Library/Application Support/Google/Chrome");
    std::fs::create_dir_all(&profile).unwrap();
    let mut chrome = Package::new(Manager::HomebrewCask, "google-chrome", "Google Chrome");
    chrome.zap = vec!["~/Library/Application Support/Google/Chrome".into()];
    let found = find_leftovers(&machine, &chrome, std::slice::from_ref(&chrome));
    assert!(found.iter().any(|l| l.path == profile), "{found:#?}");
    let mut other = Package::new(Manager::HomebrewCask, "google-drive", "Google Drive");
    other.zap = chrome.zap.clone();
    assert!(
        find_leftovers(&machine, &chrome, &[chrome.clone(), other]).is_empty(),
        "shared"
    );
}

#[test]
fn macos_leftovers_follow_the_bundle_identifier() {
    let machine = Machine::macos();
    let home = machine.home().unwrap();
    let library = home.join("Library");
    for folder in [
        "Application Support/Slack",
        "Caches/com.tinyspeck.slackmacgap",
        "Containers/com.tinyspeck.slackmacgap",
        "Group Containers/BQR82RBBHL.com.tinyspeck.slackmacgap",
        "Saved Application State/com.tinyspeck.slackmacgap.savedState",
        "Caches/com.apple.Safari",
    ] {
        std::fs::create_dir_all(library.join(folder)).unwrap();
    }
    std::fs::create_dir_all(library.join("Preferences")).unwrap();
    std::fs::write(
        library.join("Preferences/com.tinyspeck.slackmacgap.plist"),
        b"p",
    )
    .unwrap();
    let mut slack = Package::new(Manager::MacApp, "/Applications/Slack.app", "Slack");
    slack.bundle_id = Some("com.tinyspeck.slackmacgap".into());
    let found = find_leftovers(&machine, &slack, std::slice::from_ref(&slack));
    assert_eq!(found.len(), 6, "{found:#?}");
    assert!(!found.iter().any(|l| l.path.ends_with("com.apple.Safari")));
}

#[test]
fn nothing_is_offered_while_another_copy_of_the_app_stays() {
    let machine = Machine::macos();
    let library = machine.home().unwrap().join("Library");
    for folder in [
        "Caches/com.tinyspeck.slackmacgap.ShipIt",
        "Containers/com.tinyspeck.slackmacgap.helper",
    ] {
        std::fs::create_dir_all(library.join(folder)).unwrap();
    }
    let copy = |path: &str| {
        let mut slack = Package::new(Manager::MacApp, path, "Slack");
        slack.bundle_id = Some("com.tinyspeck.slackmacgap".into());
        slack
    };
    let removed = copy("/Applications/Slack.app");
    let kept = copy("/Users/me/Applications/Slack.app");
    let found = find_leftovers(&machine, &removed, &[removed.clone(), kept.clone()]);
    assert!(found.is_empty(), "{found:#?}");
    let mut cask = Package::new(Manager::HomebrewCask, "slack", "Slack");
    cask.bundle_id = kept.bundle_id.clone();
    cask.zap = vec!["~/Library/Caches/com.tinyspeck.slackmacgap.ShipIt".into()];
    let found = find_leftovers(&machine, &cask, &[cask.clone(), kept]);
    assert!(found.is_empty(), "the cask's zap list: {found:#?}");
    // The copy that stays may go by another name than the cask's token.
    std::fs::create_dir_all(library.join("Caches/zoom")).unwrap();
    let mut zoom = Package::new(Manager::HomebrewCask, "zoom", "Zoom");
    zoom.bundle_id = Some("us.zoom.xos".into());
    let mut other = Package::new(
        Manager::MacApp,
        "/Users/me/Applications/zoom.us.app",
        "zoom.us",
    );
    other.bundle_id = Some("us.zoom.xos".into());
    let found = find_leftovers(&machine, &zoom, &[zoom.clone(), other]);
    assert!(found.is_empty(), "by name: {found:#?}");
}
