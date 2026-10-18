//! The files, databases and registry entries the test machine starts with:
//! a little of everything the built-in cleaners look for, plus things that
//! must survive (a fresh temporary file, a kept folder, bookmarks).

use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use rusqlite::Connection;

use crate::fake::{MARKER, State, fold};
use crate::store;

/// Written old enough for "older than a day" filters.
fn old_file(path: &Path, bytes: usize) -> io::Result<()> {
    file(path, bytes)?;
    let old = SystemTime::now() - Duration::from_secs(10 * 86_400);
    File::options().write(true).open(path)?.set_modified(old)
}

/// Dates a folder back, as one whose contents have not changed for days.
fn age_folder(path: &Path) -> io::Result<()> {
    let old = SystemTime::now() - Duration::from_secs(10 * 86_400);
    let mut options = File::options();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_WRITE_ATTRIBUTES, and FILE_FLAG_BACKUP_SEMANTICS to open a folder.
        options.access_mode(0x0100).custom_flags(0x0200_0000);
    }
    #[cfg(not(windows))]
    options.read(true);
    options.open(path)?.set_modified(old)
}

fn file(path: &Path, bytes: usize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, vec![b'x'; bytes])
}

fn database(path: &Path, sql: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path).map_err(io::Error::other)?;
    connection.execute_batch(sql).map_err(io::Error::other)
}

/// A custom cleaner the test machine ships, proving custom files load.
fn custom_cleaner() -> String {
    format!(
        r#"[[cleaner]]
id = "northwind"
name = "{MARKER}"
description = "A browser that only exists on the test machine"
category = "browsers"
detect = ['%LOCALAPPDATA%\Northwind']

[[cleaner.option]]
id = "cache"
name = "Cache"
description = "Northwind's cache"
actions = [{{ op = "delete", search = "walk.all", path = '%LOCALAPPDATA%\Northwind\Cache' }}]
"#
    )
}

pub fn seed(sandbox: &Path) -> io::Result<()> {
    let root = sandbox.join("machine");
    let c = root.join("C");
    let profile = c.join(r"Users\Tester");
    windows(&c, &profile)?;
    browsers(&profile)?;
    applications(&profile)?;
    store::save(&root.join("state.json"), &state())?;
    let cleaners = sandbox.join("cleaners");
    fs::create_dir_all(&cleaners)?;
    fs::write(cleaners.join("northwind.toml"), custom_cleaner())
}

fn windows(c: &Path, profile: &Path) -> io::Result<()> {
    let local = profile.join(r"AppData\Local");
    let roaming = profile.join(r"AppData\Roaming");
    // Temporary files: two old ones go, a fresh one and a kept folder stay.
    old_file(&local.join(r"Temp\setup-1234.tmp"), 40_000)?;
    old_file(&local.join(r"Temp\old-installer\payload.bin"), 60_000)?;
    age_folder(&local.join(r"Temp\old-installer"))?;
    file(&local.join(r"Temp\in-use-today.tmp"), 1_000)?;
    old_file(&local.join(r"Temp\keep-me\important.tmp"), 2_000)?;
    old_file(&c.join(r"Windows\Temp\update.tmp"), 30_000)?;
    file(&c.join(r"Windows\Prefetch\NOTEPAD.EXE-1234ABCD.pf"), 9_000)?;
    file(&c.join(r"Windows\Minidump\101825-1.dmp"), 120_000)?;
    file(
        &local.join(r"Microsoft\Windows\WER\ReportArchive\AppCrash_x\Report.wer"),
        5_000,
    )?;
    file(&c.join(r"$Recycle.Bin\S-1-5-21-1\$RAB12CD.txt"), 25_000)?;
    file(
        &roaming.join(r"Microsoft\Windows\Recent\report.docx.lnk"),
        700,
    )?;
    file(
        &local.join(r"Microsoft\Windows\Explorer\thumbcache_256.db"),
        50_000,
    )?;
    Ok(())
}

fn browsers(profile: &Path) -> io::Result<()> {
    let local = profile.join(r"AppData\Local");
    let roaming = profile.join(r"AppData\Roaming");
    // Google Chrome, one profile.
    let chrome = local.join(r"Google\Chrome\User Data");
    for (name, size) in [("data_0", 8_192), ("data_1", 270_336), ("f_000001", 51_000)] {
        file(&chrome.join(r"Default\Cache\Cache_Data").join(name), size)?;
    }
    file(&chrome.join(r"Default\Code Cache\js\index"), 24_000)?;
    file(&chrome.join(r"Default\GPUCache\data_0"), 8_192)?;
    file(&chrome.join(r"Default\Network\Cookies"), 20_480)?;
    file(&chrome.join(r"Crashpad\reports\a1b2.dmp"), 300_000)?;
    file(&chrome.join(r"Default\Preferences"), 10)?;
    fs::write(
        chrome.join(r"Default\Preferences"),
        br#"{"browser":{"window_placement":{}}}"#,
    )?;
    database(
        &chrome.join(r"Default\History"),
        r"CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT);
         CREATE TABLE visits (id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
         CREATE TABLE downloads (id INTEGER PRIMARY KEY, target_path TEXT);
         CREATE TABLE downloads_url_chains (id INTEGER, url TEXT);
         INSERT INTO urls (url, title) VALUES ('https://example.com/', 'Example'), ('https://example.org/', 'Org');
         INSERT INTO visits (url, visit_time) VALUES (1, 1), (2, 2), (2, 3);
         INSERT INTO downloads (target_path) VALUES ('C:\Users\Tester\Downloads\file.zip');",
    )?;
    database(
        &chrome.join(r"Default\Web Data"),
        "CREATE TABLE autofill (name TEXT, value TEXT);
         CREATE TABLE keywords (id INTEGER PRIMARY KEY, short_name TEXT, safe_for_autoreplace INTEGER, prepopulate_id INTEGER);
         CREATE TABLE meta (key TEXT, value TEXT);
         INSERT INTO autofill VALUES ('email', 'tester@example.com');
         INSERT INTO keywords VALUES (1, 'Google', 0, 1), (2, 'some-site.example', 1, 0);
         INSERT INTO meta VALUES ('Default Search Provider ID', '1');",
    )?;

    // Firefox, one profile; the bookmarked place must survive history cleaning.
    let firefox = roaming.join(r"Mozilla\Firefox\Profiles\t3st.default-release");
    file(
        &local.join(r"Mozilla\Firefox\Profiles\t3st.default-release\cache2\entries\3F2A1B"),
        90_000,
    )?;
    file(&firefox.join("cookies.sqlite"), 32_768)?;
    file(&firefox.join("formhistory.sqlite"), 16_384)?;
    database(
        &firefox.join("places.sqlite"),
        "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, foreign_count INTEGER DEFAULT 0,
             visit_count INTEGER, last_visit_date INTEGER, origin_id INTEGER);
         CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, place_id INTEGER);
         CREATE TABLE moz_origins (id INTEGER PRIMARY KEY, host TEXT);
         CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, fk INTEGER, title TEXT);
         INSERT INTO moz_origins VALUES (1, 'bookmarked.example'), (2, 'visited.example');
         INSERT INTO moz_places VALUES (1, 'https://bookmarked.example/', 1, 4, 10, 1),
                                       (2, 'https://visited.example/', 0, 2, 11, 2);
         INSERT INTO moz_bookmarks VALUES (1, 1, 'Bookmarked');
         INSERT INTO moz_historyvisits (place_id) VALUES (1), (2), (2);",
    )?;
    Ok(())
}

fn applications(profile: &Path) -> io::Result<()> {
    let local = profile.join(r"AppData\Local");
    let roaming = profile.join(r"AppData\Roaming");
    fs::create_dir_all(roaming.join("vlc"))?;
    fs::write(
        roaming.join(r"vlc\vlc-qt-interface.ini"),
        "[General]\r\nqt-mode=1\r\n[RecentsMRU]\r\nlist=C:/Users/Tester/Videos/holiday.mp4\r\ntimes=0\r\n",
    )?;
    file(&roaming.join(r"vlc\art\artistalbum\cover.jpg"), 45_000)?;
    file(&roaming.join(r"discord\Cache\Cache_Data\data_1"), 150_000)?;
    file(&local.join(r"Northwind\Cache\page.bin"), 33_000)?;

    // Deep scan finds these; the document beside them stays.
    file(&profile.join(r"Documents\report.docx"), 12_000)?;
    file(&profile.join(r"Documents\report.docx.bak"), 11_000)?;
    file(&profile.join(r"Pictures\Holiday\Thumbs.db"), 7_000)?;

    // Build output: the target folder beside Cargo.toml goes; the project's
    // own files and a folder that only shares the name stay.
    file(&profile.join(r"Projects\demo\Cargo.toml"), 200)?;
    file(&profile.join(r"Projects\demo\src\main.rs"), 300)?;
    file(
        &profile.join(r"Projects\demo\target\debug\demo.exe"),
        90_000,
    )?;
    file(&profile.join(r"Projects\notes\target\plan.txt"), 1_000)?;

    // For Custom paths, Shred and the keep list.
    file(&profile.join(r"Documents\Scratch\draft.txt"), 4_000)?;
    file(&profile.join(r"Documents\Secret\plan.txt"), 6_000)?;
    Ok(())
}

/// Registry entries, a full clipboard and a running Firefox.
fn state() -> State {
    let mut state = State {
        clipboard: true,
        running: vec!["firefox.exe".to_string()],
        ..State::default()
    };
    for (key, values) in [
        (
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
            &["a", "b", "MRUList"][..],
        ),
        (
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\RecentDocs",
            &["MRUListEx"][..],
        ),
        (
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths",
            &["url1"][..],
        ),
        (
            r"HKCU\Software\7-Zip\Compression",
            &["ArcHistory", "Level"][..],
        ),
        (
            r"HKCU\Software\7-Zip\FM",
            &["FolderHistory", "PathHistory", "ListMode"][..],
        ),
        (
            r"HKCU\Software\Microsoft\Terminal Server Client\Default",
            &["MRU0"][..],
        ),
    ] {
        if let Some(folded) = fold(key) {
            state
                .registry
                .insert(folded, values.iter().map(|v| (*v).to_string()).collect());
        }
    }
    state
}
