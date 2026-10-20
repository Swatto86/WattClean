//! Newer Linux and macOS options, planned on a made-up home folder: what
//! each one finds, and what it must leave alone.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::SystemTime;

use super::unix_tests::{UNIX, cleaner, option};
use super::*;
use crate::model::{Action, Cleaner, CleanerOption};
use crate::paths::Guard;
use crate::search::{Filter, Findings, Searcher};

fn touch(home: &Path, files: &[&str]) {
    for file in files {
        let path = home.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"12345").unwrap();
    }
}

/// What an option's file actions find below `home`, relative to it. The
/// account's Darwin cache folder is `home/var/C` here.
fn found(home: &Path, option: &CleanerOption) -> Vec<String> {
    let base = home.to_string_lossy().into_owned();
    let vars = move |name: &str| match name {
        "HOME" => vec![base.clone()],
        "CONFIG" => vec![format!("{base}/Library/Application Support")],
        "DARWIN_USER_CACHE" => vec![format!("{base}/var/C")],
        _ => Vec::new(),
    };
    let mut found = Vec::new();
    for action in &option.actions {
        let (Action::Delete(spec) | Action::Sqlite { files: spec, .. }) = action else {
            continue;
        };
        let filter = Filter::new(spec, &vars, SystemTime::now()).unwrap();
        let mut out = Findings::default();
        let searcher = Searcher {
            guard: &Guard::default(),
            cancelled: &|| false,
        };
        searcher.find(spec, &filter, &vars, &mut out);
        assert!(out.problems.is_empty(), "{}: {:?}", option.id, out.problems);
        found.extend(out.found.iter().map(|f| {
            f.path
                .strip_prefix(home)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        }));
    }
    found.sort();
    found
}

fn admin_task(option: &CleanerOption) -> Option<&'static str> {
    match option.actions.as_slice() {
        [Action::Command(task)] if task.admin => Some(task.id),
        _ => None,
    }
}

fn detects(cleaner: &Cleaner, path: &str) -> bool {
    cleaner.detect.contains(&Detect::Path(path.to_string()))
}

/// Codex never prunes its releases; only the ones `current` does not
/// name go, never its sessions or plugins.
#[test]
fn codex_keeps_the_version_in_use_and_its_own_data() {
    for platform in UNIX {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let packages = home.join(".codex/packages");
        touch(
            home,
            &[
                ".codex/packages/standalone/releases/0.157.0/bin/codex",
                ".codex/packages/standalone/releases/0.163.0/bin/codex",
                ".codex/packages/app-server-daemon/releases/0.157.0/codex",
                ".codex/packages/app-server-daemon/releases/0.159.2/codex",
                ".codex/sessions/2026/rollout.jsonl",
                ".codex/plugins/tool/plugin.json",
            ],
        );
        for (package, version) in [("standalone", "0.163.0"), ("app-server-daemon", "0.159.2")] {
            let package = packages.join(package);
            symlink(
                package.join("releases").join(version),
                package.join("current"),
            )
            .unwrap();
        }
        let codex = cleaner(platform, "codex");
        assert!(codex.running.contains(&"codex".to_string()));
        assert!(detects(&codex, "%HOME%/.codex/packages"));
        let old = option(&codex, "old_versions");
        assert!(!old.default && old.warning.is_none());
        assert_eq!(
            found(home, old),
            [
                ".codex/packages/app-server-daemon/releases/0.157.0",
                ".codex/packages/app-server-daemon/releases/0.157.0/codex",
                ".codex/packages/standalone/releases/0.157.0",
                ".codex/packages/standalone/releases/0.157.0/bin",
                ".codex/packages/standalone/releases/0.157.0/bin/codex",
            ],
            "{platform:?}"
        );
    }
}

/// Only the compilers' module caches in the account's Darwin cache
/// folder, never the live system caches beside them.
#[test]
fn macos_module_cache_leaves_the_rest_of_the_darwin_cache_folder() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    touch(
        home,
        &[
            "var/C/clang/ModuleCache/2AB/AppKit-1.pcm",
            "var/C/clang/ModuleCache/modules.timestamp",
            "var/C/org.llvm.clang.t/ModuleCache/9XY/Foundation-2.pcm",
            "var/C/com.apple.FontRegistry/fontregistry.user",
            "var/C/com.apple.dock.iconcache",
        ],
    );
    let xcode = cleaner(Platform::Macos, "xcode");
    assert!(detects(&xcode, "%DARWIN_USER_CACHE%/clang/ModuleCache"));
    for name in ["xcode", "clang", "swift-frontend"] {
        assert!(xcode.running.contains(&name.to_string()), "{name}");
    }
    let cache = option(&xcode, "module_cache");
    assert!(!cache.default && cache.warning.is_none());
    assert_eq!(
        found(home, cache),
        [
            "var/C/clang/ModuleCache/2AB",
            "var/C/clang/ModuleCache/2AB/AppKit-1.pcm",
            "var/C/clang/ModuleCache/modules.timestamp",
            "var/C/org.llvm.clang.t/ModuleCache/9XY",
            "var/C/org.llvm.clang.t/ModuleCache/9XY/Foundation-2.pcm",
        ]
    );
}

#[test]
fn macos_xcode_cleans_test_clones_documentation_and_device_logs() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    touch(
        home,
        &[
            "Library/Developer/XCTestDevices/0A1B/device.plist",
            "Library/Developer/Xcode/macOS DeviceSupport/15.0/Symbols/dyld",
            "Library/Developer/Xcode/DocumentationCache/v1/index",
            "Library/Developer/Xcode/iOS Device Logs/app.crash",
        ],
    );
    let xcode = cleaner(Platform::Macos, "xcode");
    let has = |id: &str, path: &str| found(home, option(&xcode, id)).iter().any(|f| f == path);
    assert!(has(
        "simulator_caches",
        "Library/Developer/XCTestDevices/0A1B/device.plist"
    ));
    assert!(has(
        "device_support",
        "Library/Developer/Xcode/macOS DeviceSupport/15.0/Symbols/dyld"
    ));
    assert!(option(&xcode, "documentation").default);
    assert!(has(
        "documentation",
        "Library/Developer/Xcode/DocumentationCache/v1/index"
    ));
    let logs = option(&xcode, "device_logs");
    assert!(!logs.default);
    assert!(
        logs.warning
            .as_deref()
            .is_some_and(|w| w.contains("Organizer"))
    );
    assert_eq!(
        found(home, logs),
        ["Library/Developer/Xcode/iOS Device Logs/app.crash"]
    );
}

/// Only the firmware files: never the folders, other files, or the
/// device backups beside them.
#[test]
fn macos_device_firmware_goes_and_backups_stay() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    touch(
        home,
        &[
            "Library/iTunes/iPhone Software Updates/iPhone17,1_26.0_Restore.ipsw",
            "Library/iTunes/iPad Software Updates/iPad16,3_26.0_Restore.ipsw",
            "Library/iTunes/iPhone Software Updates/notes.txt",
            "Library/Application Support/MobileSync/Backup/00008/Manifest.db",
        ],
    );
    let devices = cleaner(Platform::Macos, "apple_devices");
    let firmware = option(&devices, "firmware");
    assert!(firmware.default && firmware.warning.is_none());
    assert_eq!(
        found(home, firmware),
        [
            "Library/iTunes/iPad Software Updates/iPad16,3_26.0_Restore.ipsw",
            "Library/iTunes/iPhone Software Updates/iPhone17,1_26.0_Restore.ipsw",
        ]
    );
}

/// `.build` beside a Package.swift, in a project anywhere in the home
/// folder, never a `.build` on its own or in a hidden folder.
#[test]
fn swift_build_folders_are_found_only_beside_a_package() {
    for platform in UNIX {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        touch(
            home,
            &[
                "code/app/Package.swift",
                "code/app/.build/debug/app",
                "code/app/Sources/main.swift",
                "code/loose/.build/x",
                ".swiftpm/pkg/Package.swift",
                ".swiftpm/pkg/.build/y",
            ],
        );
        let swift = option(&cleaner(platform, "build_output"), "swift").clone();
        assert!(swift.warning.is_some() && !swift.default);
        assert_eq!(
            found(home, &swift),
            [
                "code/app/.build",
                "code/app/.build/debug",
                "code/app/.build/debug/app"
            ],
            "{platform:?}"
        );
    }
}

#[test]
fn macos_adobe_media_cache_warns_and_keeps_the_rest_of_adobes_folder() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let common = "Library/Application Support/Adobe/Common";
    touch(
        home,
        &[
            &format!("{common}/Media Cache Files/clip.cfa"),
            &format!("{common}/Media Cache/clip.pek"),
            &format!("{common}/Plug-ins/7.0/MediaCore/a.bundle"),
        ],
    );
    let adobe = cleaner(Platform::Macos, "adobe_media");
    assert!(!adobe.running.is_empty());
    let cache = option(&adobe, "media_cache");
    assert!(cache.warning.is_some() && !cache.default);
    assert_eq!(
        found(home, cache),
        [
            format!("{common}/Media Cache Files/clip.cfa"),
            format!("{common}/Media Cache/clip.pek"),
        ]
    );
}

#[test]
fn macos_download_records_empty_the_quarantine_list() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let file = "Library/Preferences/com.apple.LaunchServices.QuarantineEventsV2";
    touch(home, &[file, "Library/Preferences/com.apple.finder.plist"]);
    let records = option(&cleaner(Platform::Macos, "system"), "download_records").clone();
    assert!(!records.default);
    let [Action::Sqlite { op, .. }] = records.actions.as_slice() else {
        panic!("{:?}", records.actions);
    };
    assert_eq!(op.label(), "download records");
    assert_eq!(found(home, &records), [file]);
}

/// What asks for an administrator's password is never ticked by itself,
/// and what loses history says so.
#[test]
fn macos_administrator_options_are_never_default() {
    let system = cleaner(Platform::Macos, "system");
    for (id, task, warns) in [
        ("local_snapshots", "tm-snapshots", true),
        ("system_logs", "log-erase", true),
        ("dns_cache", "dns-flush", false),
    ] {
        let chosen = option(&system, id);
        assert_eq!(admin_task(chosen), Some(task), "{id}");
        assert_eq!(chosen.warning.is_some(), warns, "{id}");
    }
    for chosen in &system.options {
        assert!(
            !(chosen.default && admin_task(chosen).is_some()),
            "system.{} asks for a password by default",
            chosen.id
        );
    }
}
