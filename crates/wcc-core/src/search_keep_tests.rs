//! `keep_link`: every release but the one a link names, as Codex keeps
//! them (`<package>/current` links to `<package>/releases/<version>`).
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::SystemTime;

use super::*;
use crate::defs::parse_files;
use crate::model::{Action, Source};

/// The one action of a definition, as a custom file would give it.
fn spec(action: &str) -> FileSpec {
    let text = format!(
        "[[cleaner]]\nid = \"a\"\nname = \"A\"\ncategory = \"developer\"\n\
         [[cleaner.option]]\nid = \"x\"\nname = \"X\"\nactions = [{action}]\n"
    );
    let (cleaners, problems) = parse_files(&[("k.toml", &text)], Source::Custom);
    assert!(problems.is_empty(), "{problems:?}");
    match &cleaners[0].options[0].actions[0] {
        Action::Delete(spec) => spec.clone(),
        other => panic!("{other:?}"),
    }
}

const OLD_RELEASES: &str = r#"{ op = "delete", search = "walk.top", path = '%ROOT%/packages/*/releases/*', keep_link = '../current' }"#;

fn release(root: &Path, package: &str, version: &str) {
    let bin = root
        .join("packages")
        .join(package)
        .join("releases")
        .join(version);
    fs::create_dir_all(bin.join("bin")).unwrap();
    fs::write(bin.join("bin").join("tool"), b"12345").unwrap();
}

/// Points `<package>/current` at `target` (replacing the link there).
fn current(root: &Path, package: &str, target: &Path) {
    let link = root.join("packages").join(package).join("current");
    let _gone = fs::remove_file(&link);
    symlink(target, link).unwrap();
}

/// What the search finds below `root`, relative to it, and its problems.
fn find(root: &Path, spec: &FileSpec) -> (Vec<String>, Vec<Problem>) {
    let base = root.to_string_lossy().into_owned();
    let vars = move |name: &str| {
        if name == "ROOT" {
            vec![base.clone()]
        } else {
            Vec::new()
        }
    };
    let filter = Filter::new(spec, &vars, SystemTime::now()).unwrap();
    let never = || false;
    let searcher = Searcher {
        guard: &Guard::default(),
        cancelled: &never,
    };
    let mut out = Findings::default();
    searcher.find(spec, &filter, &vars, &mut out);
    let mut found: Vec<String> = out
        .found
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    found.sort();
    (found, out.problems)
}

fn whole(release: &str) -> [String; 3] {
    [
        release.to_string(),
        format!("{release}/bin"),
        format!("{release}/bin/tool"),
    ]
}

/// The link may be absolute (as Codex writes it) or relative; what it
/// names, and the link itself, stay.
#[test]
fn every_release_but_the_one_in_use_goes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for version in ["1.0", "1.1", "1.2"] {
        release(root, "standalone", version);
    }
    current(
        root,
        "standalone",
        &root.join("packages/standalone/releases/1.2"),
    );
    for version in ["1.0", "1.1"] {
        release(root, "daemon", version);
    }
    current(root, "daemon", Path::new("releases/1.0"));
    let (found, problems) = find(root, &spec(OLD_RELEASES));
    assert!(problems.is_empty(), "{problems:?}");
    let mut expected: Vec<String> = [
        "packages/daemon/releases/1.1",
        "packages/standalone/releases/1.0",
        "packages/standalone/releases/1.1",
    ]
    .iter()
    .flat_map(|r| whole(r))
    .collect();
    expected.sort();
    assert_eq!(found, expected);
}

/// Without a link that names one of the folder's items, which one is in
/// use is not known: nothing there is taken, and the report says why.
#[test]
fn a_missing_or_broken_link_keeps_everything_in_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let elsewhere = tempfile::tempdir().unwrap();
    for package in ["none", "broken", "folder", "outside", "fine"] {
        release(root, package, "1.0");
        release(root, package, "1.1");
    }
    current(root, "broken", &root.join("packages/broken/releases/9.9"));
    fs::create_dir(root.join("packages/folder/current")).unwrap();
    current(root, "outside", elsewhere.path());
    current(root, "fine", Path::new("releases/1.1"));
    let (found, problems) = find(root, &spec(OLD_RELEASES));
    assert_eq!(found, whole("packages/fine/releases/1.0"));
    // Once for each folder, not once for each release in it.
    for package in ["none", "broken", "folder", "outside"] {
        let folder = root.join("packages").join(package).join("releases");
        let said: Vec<_> = problems.iter().filter(|p| p.path == folder).collect();
        assert!(
            said.len() == 1 && said[0].message.contains("../current"),
            "{package}: {problems:?}"
        );
    }
    assert_eq!(problems.len(), 4, "{problems:?}");
}

/// A clean plans again, so the link is read again: an update between a
/// preview and a clean keeps the new version, not the one previewed.
#[test]
fn the_link_is_read_again_each_time() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for version in ["1.0", "1.1"] {
        release(root, "standalone", version);
    }
    current(root, "standalone", Path::new("releases/1.0"));
    let spec = spec(OLD_RELEASES);
    assert_eq!(
        find(root, &spec).0,
        whole("packages/standalone/releases/1.1")
    );
    current(root, "standalone", Path::new("releases/1.1"));
    assert_eq!(
        find(root, &spec).0,
        whole("packages/standalone/releases/1.0")
    );
}

/// A folder holding the version in use, or the link, is never taken whole.
#[test]
fn a_folder_holding_the_version_in_use_or_its_link_stays() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for version in ["1.0", "1.1"] {
        release(root, "standalone", version);
    }
    current(root, "standalone", Path::new("releases/1.1"));
    let holding = spec(
        r#"{ op = "delete", search = "walk.top", path = '%ROOT%/packages/*/releases', keep_link = 'current' }"#,
    );
    let (found, problems) = find(root, &holding);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(found, whole("packages/standalone/releases/1.0"));
    let beside = spec(
        r#"{ op = "delete", search = "walk.top", path = '%ROOT%/packages/standalone/*', keep_link = 'current' }"#,
    );
    assert_eq!(
        find(root, &beside).0,
        whole("packages/standalone/releases/1.0")
    );
}

/// The link is named from each found item's folder: never a full path or
/// a variable, which could name anything.
#[test]
fn keep_link_is_a_relative_path() {
    for link in ["/opt/current", "%HOME%/current", "C:/current", ""] {
        let text = format!(
            "[[cleaner]]\nid = \"a\"\nname = \"A\"\ncategory = \"developer\"\n\
             [[cleaner.option]]\nid = \"x\"\nname = \"X\"\nactions = [{{ op = \"delete\", \
             search = \"walk.top\", path = '%HOME%/r/*', keep_link = '{link}' }}]\n"
        );
        let (cleaners, problems) = parse_files(&[("k.toml", &text)], Source::Custom);
        assert!(cleaners.is_empty(), "{link}");
        assert!(
            problems[0]
                .to_string()
                .contains("keep_link must be a path relative"),
            "{link}: {}",
            problems[0]
        );
    }
}
