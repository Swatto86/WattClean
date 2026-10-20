//! Work inside browser databases (and macOS's download records): removing
//! history, downloads, form entries and site search engines while keeping
//! the rest (bookmarks stay), and vacuuming to give back the space deleted
//! rows leave.
//!
//! Browsers change their schemas, so every statement is tried on its own and
//! a missing table or column is not an error. A database the browser has
//! open reports "in use", like a locked file.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::model::SqliteOp;

/// Whether the file starts with SQLite's header.
pub fn is_database(path: &Path) -> bool {
    let mut header = [0u8; 16];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_ok_and(|()| &header == b"SQLite format 3\0")
}

fn statements(op: SqliteOp) -> &'static [&'static str] {
    match op {
        SqliteOp::Vacuum => &[],
        SqliteOp::ChromeHistory => &[
            "DELETE FROM visits",
            "DELETE FROM visit_source",
            "DELETE FROM keyword_search_terms",
            "DELETE FROM segment_usage",
            "DELETE FROM segments",
            "DELETE FROM visited_links",
            "DELETE FROM clusters_and_visits",
            "DELETE FROM clusters",
            "DELETE FROM cluster_keywords",
            "DELETE FROM cluster_visit_duplicates",
            "DELETE FROM content_annotations",
            "DELETE FROM context_annotations",
            "DELETE FROM urls",
        ],
        SqliteOp::ChromeDownloads => &[
            "DELETE FROM downloads_url_chains",
            "DELETE FROM downloads_slices",
            "DELETE FROM downloads_reroute_info",
            "DELETE FROM downloads",
        ],
        SqliteOp::ChromeAutofill => &[
            "DELETE FROM autofill",
            "DELETE FROM autofill_profiles",
            "DELETE FROM autofill_profile_names",
            "DELETE FROM autofill_profile_emails",
            "DELETE FROM autofill_profile_phones",
            "DELETE FROM autofill_profile_addresses",
            "DELETE FROM local_addresses",
            "DELETE FROM local_addresses_type_tokens",
        ],
        // Engines sites added on their own, never the default or built-in ones.
        SqliteOp::ChromeKeywords => &[
            "DELETE FROM keywords WHERE safe_for_autoreplace = 1 AND prepopulate_id = 0 \
             AND id <> COALESCE((SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'Default Search Provider ID'), -1)",
        ],
        SqliteOp::MozillaHistory => &[
            "DELETE FROM moz_historyvisits",
            "DELETE FROM moz_inputhistory",
            "DELETE FROM moz_places_metadata_search_queries",
            "DELETE FROM moz_places_metadata",
            // Bookmarked and keyword places are counted in foreign_count.
            "DELETE FROM moz_places WHERE foreign_count = 0",
            "UPDATE moz_places SET visit_count = 0, last_visit_date = NULL",
            "DELETE FROM moz_annos WHERE place_id NOT IN (SELECT id FROM moz_places)",
            "DELETE FROM moz_origins WHERE id NOT IN (SELECT origin_id FROM moz_places)",
        ],
        SqliteOp::MozillaDownloads => &["DELETE FROM moz_annos WHERE anno_attribute_id IN \
             (SELECT id FROM moz_anno_attributes WHERE name LIKE 'downloads/%')"],
        // Rows only: LaunchServices keeps adding to the table.
        SqliteOp::QuarantineEvents => &["DELETE FROM LSQuarantineEvent"],
    }
}

fn to_io(error: rusqlite::Error) -> io::Error {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
            io::Error::from_raw_os_error(32)
        }
        _ => io::Error::other(error.to_string()),
    }
}

fn schema_mismatch(error: &rusqlite::Error) -> bool {
    let text = error.to_string();
    text.contains("no such table") || text.contains("no such column")
}

/// Runs the operation and vacuums; returns the bytes the file shrank by.
pub fn run(path: &Path, op: SqliteOp) -> io::Result<u64> {
    let before = fs::metadata(path)?.len();
    {
        let connection =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(to_io)?;
        connection
            .busy_timeout(Duration::from_millis(500))
            .map_err(to_io)?;
        // Taking the write lock first turns a browser that has the file open
        // into one clear "in use" instead of half-done deletes.
        connection.execute_batch("BEGIN IMMEDIATE").map_err(to_io)?;
        for statement in statements(op) {
            if let Err(error) = connection.execute_batch(statement)
                && !schema_mismatch(&error)
            {
                let _rolled_back = connection.execute_batch("ROLLBACK");
                return Err(to_io(error));
            }
        }
        connection.execute_batch("COMMIT").map_err(to_io)?;
        connection.execute_batch("VACUUM").map_err(to_io)?;
    }
    let after = fs::metadata(path)?.len();
    Ok(before.saturating_sub(after))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(path: &Path, table: &str) -> i64 {
        let connection = Connection::open(path).unwrap();
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn chrome_history_goes_and_other_tables_stay() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("History");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT);
                 CREATE TABLE visits (id INTEGER PRIMARY KEY, url INTEGER);
                 CREATE TABLE downloads (id INTEGER PRIMARY KEY);
                 CREATE TABLE meta (key TEXT, value TEXT);
                 INSERT INTO urls (url) VALUES ('https://example.com'), ('https://example.org');
                 INSERT INTO visits (url) VALUES (1), (2), (2);
                 INSERT INTO downloads DEFAULT VALUES;
                 INSERT INTO meta VALUES ('version', '1');",
            )
            .unwrap();
        drop(connection);
        assert!(is_database(&path));
        run(&path, SqliteOp::ChromeHistory).unwrap();
        assert_eq!(count(&path, "urls"), 0);
        assert_eq!(count(&path, "visits"), 0);
        assert_eq!(
            count(&path, "downloads"),
            1,
            "downloads are a separate option"
        );
        assert_eq!(count(&path, "meta"), 1);
    }

    #[test]
    fn firefox_history_keeps_bookmarked_places() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("places.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, foreign_count INTEGER,
                     visit_count INTEGER, last_visit_date INTEGER, origin_id INTEGER);
                 CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, place_id INTEGER);
                 CREATE TABLE moz_origins (id INTEGER PRIMARY KEY);
                 INSERT INTO moz_origins (id) VALUES (1), (2);
                 INSERT INTO moz_places VALUES (1, 'https://kept.example', 1, 5, 99, 1),
                                               (2, 'https://visited.example', 0, 3, 98, 2);
                 INSERT INTO moz_historyvisits (place_id) VALUES (1), (2);",
            )
            .unwrap();
        drop(connection);
        run(&path, SqliteOp::MozillaHistory).unwrap();
        assert_eq!(count(&path, "moz_places"), 1);
        assert_eq!(count(&path, "moz_historyvisits"), 0);
        assert_eq!(count(&path, "moz_origins"), 1);
        let connection = Connection::open(&path).unwrap();
        let visits: i64 = connection
            .query_row("SELECT visit_count FROM moz_places", [], |r| r.get(0))
            .unwrap();
        assert_eq!(visits, 0);
    }

    /// macOS's download records: every row goes, the table stays for
    /// LaunchServices to add to.
    #[test]
    fn download_records_go_and_their_table_stays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("com.apple.LaunchServices.QuarantineEventsV2");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE LSQuarantineEvent (LSQuarantineEventIdentifier TEXT PRIMARY KEY NOT NULL,
                     LSQuarantineTimeStamp REAL, LSQuarantineAgentName TEXT,
                     LSQuarantineDataURLString TEXT);
                 INSERT INTO LSQuarantineEvent VALUES ('A', 1.0, 'Safari', 'https://example.com/a.dmg'),
                                                      ('B', 2.0, 'Messages', NULL);",
            )
            .unwrap();
        drop(connection);
        assert!(is_database(&path));
        run(&path, SqliteOp::QuarantineEvents).unwrap();
        assert_eq!(count(&path, "LSQuarantineEvent"), 0);
    }

    #[test]
    fn a_locked_database_is_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Web Data");
        let holder = Connection::open(&path).unwrap();
        holder.execute_batch("CREATE TABLE autofill (name TEXT); BEGIN EXCLUSIVE; INSERT INTO autofill VALUES ('x');").unwrap();
        let error = run(&path, SqliteOp::ChromeAutofill).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(32));
        holder.execute_batch("COMMIT").unwrap();
    }

    #[test]
    fn other_files_are_not_databases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Preferences");
        fs::write(&path, b"{\"not\": \"sqlite\"}").unwrap();
        assert!(!is_database(&path));
    }
}
