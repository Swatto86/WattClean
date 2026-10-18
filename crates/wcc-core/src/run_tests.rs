use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;
use crate::catalog::Catalog;
use crate::fake::FakeSystem;
use crate::model::OptionKey;
use crate::ops::CustomPath;
use crate::report::{Item, Mode, Progress, Sink};
use crate::system::System;

struct Machine {
    _dir: tempfile::TempDir,
    system: FakeSystem,
    catalog: Catalog,
}

fn machine() -> Machine {
    let dir = tempfile::tempdir().unwrap();
    let sandbox = dir.path().to_path_buf();
    let system = FakeSystem::open(&sandbox).unwrap();
    let catalog = Catalog::load(Some(&sandbox.join("cleaners")), false);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    Machine {
        _dir: dir,
        system,
        catalog,
    }
}

impl Machine {
    fn path(&self, rel: &str) -> PathBuf {
        self.system.drive_c().join(rel)
    }

    fn run(&self, keys: &[&str], mode: Mode, settings: &RunSettings) -> crate::report::Report {
        let selection: Vec<OptionKey> = keys.iter().map(|k| OptionKey::parse(k).unwrap()).collect();
        run(
            &self.catalog,
            &selection,
            mode,
            &self.system,
            settings,
            &crate::report::Quiet,
        )
    }
}

const PROFILE: &str = r"Users\Tester";

#[test]
fn a_preview_lists_without_deleting() {
    let m = machine();
    let report = m.run(
        &["system.temp", "chrome.cache"],
        Mode::Preview,
        &RunSettings::default(),
    );
    assert_eq!(report.options.len(), 2);
    let temp = &report.options[0];
    let paths: Vec<_> = temp.items.iter().map(|i| i.path.clone()).collect();
    assert!(
        paths.iter().any(|p| p.ends_with("setup-1234.tmp")),
        "{paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.ends_with("old-installer")),
        "the emptied folder goes too: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.ends_with("in-use-today.tmp")),
        "a fresh file is left"
    );
    assert!(
        m.path(&format!(r"{PROFILE}\AppData\Local\Temp\setup-1234.tmp"))
            .exists()
    );
    assert_eq!(report.options[1].counts.files, 5);
    assert_eq!(
        report.totals.bytes,
        temp.counts.bytes + report.options[1].counts.bytes
    );
}

#[test]
fn a_clean_deletes_what_the_preview_listed_and_honours_the_keep_list() {
    let m = machine();
    let keep = m.path(&format!(r"{PROFILE}\AppData\Local\Temp\keep-me"));
    let settings = RunSettings {
        keep: vec![keep.clone()],
        ..RunSettings::default()
    };
    let preview = m.run(&["system.temp"], Mode::Preview, &settings);
    let clean = m.run(&["system.temp"], Mode::Clean, &settings);
    assert_eq!(preview.totals, clean.totals);
    assert!(
        clean.totals.problems == 0,
        "{:?}",
        clean.options[0].problems
    );
    assert!(
        !m.path(&format!(r"{PROFILE}\AppData\Local\Temp\setup-1234.tmp"))
            .exists()
    );
    assert!(
        !m.path(&format!(r"{PROFILE}\AppData\Local\Temp\old-installer"))
            .exists()
    );
    assert!(
        m.path(&format!(r"{PROFILE}\AppData\Local\Temp\in-use-today.tmp"))
            .exists()
    );
    assert!(
        keep.join("important.tmp").exists(),
        "the keep list was ignored"
    );
    assert!(
        m.path(&format!(r"{PROFILE}\AppData\Local\Temp")).exists(),
        "the temp folder itself stays"
    );
}

#[test]
fn registry_recycle_bin_and_clipboard_are_cleaned() {
    let m = machine();
    let report = m.run(
        &[
            "explorer.run",
            "seven_zip.history",
            "system.recycle_bin",
            "system.clipboard",
        ],
        Mode::Clean,
        &RunSettings::default(),
    );
    assert_eq!(report.totals.registry, 4, "{:#?}", report.options);
    let registry = m.system.registry();
    assert!(
        !registry.key_exists(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU")
    );
    assert!(!registry.value_exists(r"HKCU\Software\7-Zip\FM", "FolderHistory"));
    assert!(
        registry.value_exists(r"HKCU\Software\7-Zip\FM", "ListMode"),
        "other values stay"
    );
    assert_eq!(m.system.recycle_bin().unwrap(), (0, 0));
    assert!(!m.system.clipboard_has_data());
}

#[test]
fn browser_databases_lose_history_but_keep_bookmarks() {
    let m = machine();
    let report = m.run(
        &["chrome.history", "firefox.history"],
        Mode::Clean,
        &RunSettings::default(),
    );
    assert_eq!(report.totals.problems, 0, "{:#?}", report.options);
    let count = |path: PathBuf, table: &str| -> i64 {
        rusqlite::Connection::open(path)
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    let chrome = m.path(&format!(
        r"{PROFILE}\AppData\Local\Google\Chrome\User Data\Default\History"
    ));
    assert_eq!(count(chrome.clone(), "urls"), 0);
    assert_eq!(count(chrome, "downloads"), 1);
    let places = m.path(&format!(
        r"{PROFILE}\AppData\Roaming\Mozilla\Firefox\Profiles\t3st.default-release\places.sqlite"
    ));
    assert_eq!(count(places.clone(), "moz_places"), 1);
    assert_eq!(count(places, "moz_historyvisits"), 0);
    assert_eq!(
        report.running,
        vec!["Firefox".to_string()],
        "the test machine runs Firefox"
    );
}

#[test]
fn ini_edits_and_custom_cleaners_work() {
    let m = machine();
    let report = m.run(
        &["vlc.recent", "northwind.cache"],
        Mode::Clean,
        &RunSettings::default(),
    );
    assert_eq!(report.options.len(), 2);
    let ini = std::fs::read_to_string(m.path(&format!(
        r"{PROFILE}\AppData\Roaming\vlc\vlc-qt-interface.ini"
    )))
    .unwrap();
    assert!(
        !ini.contains("RecentsMRU") && ini.contains("qt-mode=1"),
        "{ini}"
    );
    assert!(
        !m.path(&format!(
            r"{PROFILE}\AppData\Local\Northwind\Cache\page.bin"
        ))
        .exists()
    );
}

#[test]
fn custom_paths_and_shredding() {
    let m = machine();
    let scratch = m.path(&format!(r"{PROFILE}\Documents\Scratch"));
    let settings = RunSettings {
        custom_paths: vec![CustomPath {
            path: scratch.to_string_lossy().to_string(),
            folder: true,
        }],
        shred: true,
        ..RunSettings::default()
    };
    let report = m.run(&["system.custom"], Mode::Clean, &settings);
    assert_eq!(
        (report.totals.files, report.totals.folders),
        (1, 1),
        "{:#?}",
        report.options
    );
    assert!(!scratch.exists());
}

#[test]
fn a_repeated_option_runs_once() {
    let m = machine();
    let report = m.run(
        &["system.temp", "system.temp", "deep_scan.backup"],
        Mode::Preview,
        &RunSettings::default(),
    );
    assert_eq!(report.options.len(), 2, "a repeated key runs once");
}

#[test]
fn deep_scan_finds_leftovers_by_name() {
    let m = machine();
    let report = m.run(
        &["deep_scan.backup", "deep_scan.thumbs_db"],
        Mode::Preview,
        &RunSettings::default(),
    );
    let names: Vec<_> = report
        .options
        .iter()
        .flat_map(|o| {
            o.items
                .iter()
                .map(|i| i.path.rsplit('\\').next().unwrap_or_default().to_string())
        })
        .collect();
    assert_eq!(
        names,
        vec!["report.docx.bak".to_string(), "Thumbs.db".to_string()]
    );
}

struct Counting {
    items: AtomicUsize,
    progress: AtomicUsize,
    stop_after: usize,
    stopped: AtomicBool,
}

impl Sink for Counting {
    fn progress(&self, _progress: &Progress) {
        self.progress.fetch_add(1, Ordering::Relaxed);
    }
    fn item(&self, _key: &OptionKey, _item: &Item) {
        if self.items.fetch_add(1, Ordering::Relaxed) + 1 >= self.stop_after {
            self.stopped.store(true, Ordering::Relaxed);
        }
    }
    fn cancelled(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

#[test]
fn a_run_reports_progress_and_stops_when_cancelled() {
    let m = machine();
    let sink = Counting {
        items: AtomicUsize::new(0),
        progress: AtomicUsize::new(0),
        stop_after: 2,
        stopped: AtomicBool::new(false),
    };
    let selection: Vec<OptionKey> = ["chrome.cache", "system.temp"]
        .iter()
        .map(|k| OptionKey::parse(k).unwrap())
        .collect();
    let report = run(
        &m.catalog,
        &selection,
        Mode::Clean,
        &m.system,
        &RunSettings::default(),
        &sink,
    );
    assert!(report.cancelled);
    assert_eq!(report.totals.files, 2);
    assert!(sink.progress.load(Ordering::Relaxed) >= 2);
    assert!(
        m.path(&format!(r"{PROFILE}\AppData\Local\Temp\setup-1234.tmp"))
            .exists(),
        "stopped before temp"
    );
}

#[test]
fn free_disk_space_runs_after_everything_else_and_cleans_up() {
    let m = machine();
    let report = m.run(
        &["system.free_disk_space", "system.temp"],
        Mode::Clean,
        &RunSettings::default(),
    );
    let keys: Vec<_> = report.options.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(keys, vec!["system.temp", "system.free_disk_space"]);
    let wipe = &report.options[1];
    assert_eq!(wipe.counts.other, 1, "{:#?}", wipe);
    assert_eq!(wipe.items[0].note.as_deref(), Some("3 MB overwritten"));
    let leftovers = std::fs::read_dir(m.system.drive_c())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("WattClean-wipe-")
        })
        .count();
    assert_eq!(leftovers, 0);
}
