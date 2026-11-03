
use super::*;

fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_string())
    }
}

#[test]
fn linux_folders_follow_xdg_and_fall_back_to_its_defaults() {
    let folders = Folders::from_env(
        Platform::Linux,
        &env(&[("HOME", "/home/t/"), ("XDG_CACHE_HOME", "/var/cache/t")]),
    );
    assert_eq!(folders.get("home"), Some("/home/t"));
    assert_eq!(folders.get("CACHE"), Some("/var/cache/t"));
    assert_eq!(folders.get("XDG_CONFIG_HOME"), Some("/home/t/.config"));
    assert_eq!(folders.get("TRASH"), Some("/home/t/.local/share/Trash"));
    assert_eq!(folders.get("TEMP"), Some("/tmp"));
    assert_eq!(folders.get("HOMEBREW_CACHE"), Some("/var/cache/t/Homebrew"));
    assert_eq!(folders.get("DOWNLOADS"), Some("/home/t/Downloads"));
    assert_eq!(folders.get("DARWIN_USER_CACHE"), None, "macOS's alone");
    for name in wcc_core::vars::UNIX_VARIABLES {
        assert!(folders.get(name).is_some(), "{name} has no value");
    }
}

#[test]
fn macos_folders_are_in_the_library() {
    let folders = Folders::from_env(
        Platform::Macos,
        &env(&[("HOME", "/Users/t"), ("TMPDIR", "/var/folders/ab/xyz/T/")]),
    );
    assert_eq!(
        folders.get("CONFIG"),
        Some("/Users/t/Library/Application Support")
    );
    assert_eq!(folders.get("CACHE"), Some("/Users/t/Library/Caches"));
    assert_eq!(folders.get("TRASH"), Some("/Users/t/.Trash"));
    assert_eq!(folders.get("TEMP"), Some("/var/folders/ab/xyz/T"));
    for name in wcc_core::vars::UNIX_VARIABLES {
        assert!(folders.get(name).is_some(), "{name} has no value");
    }
    // The account's own, beside its temporary folder: never from `HOME`.
    if cfg!(target_os = "macos") {
        let cache = folders.get("DARWIN_USER_CACHE").unwrap_or_default();
        assert!(cache.starts_with('/') && cache.ends_with("/C"), "{cache:?}");
    }
}

#[test]
fn no_home_means_no_home_folders() {
    let folders = Folders::from_env(Platform::Linux, &env(&[("HOME", "/")]));
    assert_eq!(folders.get("HOME"), None);
    assert_eq!(folders.get("CACHE"), None);
    assert_eq!(folders.get("TEMP"), Some("/tmp"));
}

#[test]
fn a_temporary_folder_that_holds_a_persons_files_is_not_trusted() {
    for tmpdir in [
        "/home/t",
        "/home/t/Documents",
        "/home/t/.ssh",
        "/home",
        "/usr",
        "/HOME/T/",
        "/home/t/..",
    ] {
        let folders = Folders::from_env(Platform::Linux, &|name| match name {
            "HOME" => Some("/home/t".to_string()),
            "TMPDIR" => Some(tmpdir.to_string()),
            _ => None,
        });
        for name in ["TEMP", "TMP", "TMPDIR"] {
            assert_eq!(folders.get(name), Some("/tmp"), "TMPDIR={tmpdir}");
        }
    }
    for tmpdir in ["/tmp", "/var/tmp", "/home/t/tmp", "/run/user/1000"] {
        let folders = Folders::from_env(Platform::Linux, &|name| match name {
            "HOME" => Some("/home/t".to_string()),
            "TMPDIR" => Some(tmpdir.to_string()),
            _ => None,
        });
        assert_eq!(folders.get("TEMP"), Some(tmpdir));
    }
}

#[test]
fn macos_never_takes_the_home_folder_or_its_documents_for_temporary() {
    for tmpdir in [
        "/Users/t",
        "/Users/t/Documents",
        "/Applications",
        "/private/var",
    ] {
        let folders = Folders::from_env(Platform::Macos, &|name| match name {
            "HOME" => Some("/Users/t".to_string()),
            "TMPDIR" => Some(tmpdir.to_string()),
            _ => None,
        });
        let temp = folders.get("TEMP").unwrap();
        assert!(temp.starts_with('/'), "{temp}");
        assert!(
            temp != "/Users/t" && !temp.starts_with("/Users/t/") && temp != tmpdir,
            "TMPDIR={tmpdir} gave {temp}"
        );
    }
}

#[test]
fn xdg_folders_that_hold_a_persons_files_fall_back_to_their_defaults() {
    let folders = Folders::from_env(
        Platform::Linux,
        &env(&[
            ("HOME", "/home/t"),
            ("XDG_CACHE_HOME", "/home/t"),
            ("XDG_DATA_HOME", "/home/t/Documents"),
            ("XDG_CONFIG_HOME", "/etc"),
            ("XDG_STATE_HOME", "/home/t"),
            ("GOPATH", "/home/t:/home/t/go"),
            ("CARGO_HOME", "/home/t"),
            ("HOMEBREW_CACHE", "/home/t"),
        ]),
    );
    assert_eq!(folders.get("CACHE"), Some("/home/t/.cache"));
    assert_eq!(folders.get("XDG_CACHE_HOME"), Some("/home/t/.cache"));
    assert_eq!(folders.get("XDG_DATA_HOME"), Some("/home/t/.local/share"));
    assert_eq!(folders.get("TRASH"), Some("/home/t/.local/share/Trash"));
    assert_eq!(folders.get("XDG_CONFIG_HOME"), Some("/home/t/.config"));
    assert_eq!(folders.get("XDG_STATE_HOME"), Some("/home/t/.local/state"));
    assert_eq!(folders.get("GOPATH"), Some("/home/t/go"));
    assert_eq!(folders.get("CARGO_HOME"), Some("/home/t/.cargo"));
    assert_eq!(
        folders.get("HOMEBREW_CACHE"),
        Some("/home/t/.cache/Homebrew")
    );
}

#[test]
fn a_moved_user_folder_is_kept_out_of_temporary_too() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(
        config.join("user-dirs.dirs"),
        "XDG_DOCUMENTS_DIR=\"/data/docs\"\n",
    )
    .unwrap();
    let config = config.to_string_lossy().into_owned();
    let folders = Folders::from_env(Platform::Linux, &|name| match name {
        "HOME" => Some("/home/t".to_string()),
        "XDG_CONFIG_HOME" => Some(config.clone()),
        "TMPDIR" => Some("/data".to_string()),
        _ => None,
    });
    assert_eq!(folders.get("DOCUMENTS"), Some("/data/docs"));
    assert_eq!(folders.get("TEMP"), Some("/tmp"));
}

#[test]
fn user_dirs_are_read_with_home_expanded() {
    let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\nXDG_MUSIC_DIR=\"$HOME/\"\nXDG_VIDEOS_DIR=\"/data/videos\"\n";
    assert_eq!(
        user_dir(text, "XDG_DESKTOP_DIR", "/home/t").as_deref(),
        Some("/home/t/Schreibtisch")
    );
    assert_eq!(user_dir(text, "XDG_MUSIC_DIR", "/home/t"), None);
    assert_eq!(
        user_dir(text, "XDG_VIDEOS_DIR", "/home/t").as_deref(),
        Some("/data/videos")
    );
    assert_eq!(user_dir(text, "XDG_DOWNLOAD_DIR", "/home/t"), None);
}
