//! The page's commands. Each validates its input, hands the work to the
//! engine off the async runtime, and returns a typed result or a sentence.

pub mod background;
pub mod backups;
pub mod cleaner;
pub mod menu;
pub mod options;
pub mod platform;
pub mod state;
pub mod tuner;
pub mod watcher;

use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, State};
use wrc_core::keyname::parse_key;
use wrc_core::model::RegPath;
use wrc_core::settings::Settings;

use crate::error::{CommandError, CommandResult, blocking};
use crate::registry::state::AppState;

pub type App<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
pub fn reg_get_settings(state: App<'_>) -> CommandResult<Settings> {
    Ok(Settings::load(&state.data_dir)?)
}

#[tauri::command]
pub fn reg_save_settings(state: App<'_>, settings: Settings) -> CommandResult<()> {
    // The backups folder changes only by moving the backups.
    let mut settings = settings;
    settings.backups_folder = Settings::load(&state.data_dir)?.backups_folder;
    settings.save(&state.data_dir)?;
    Ok(())
}

/// A key the page names, checked as a person's typing would be.
pub fn key_from_page(text: &str) -> CommandResult<RegPath> {
    parse_key(text).map_err(CommandError::from)
}

#[tauri::command]
pub async fn reg_open_in_regedit(state: App<'_>, key: String) -> CommandResult<()> {
    let key = key_from_page(&key)?;
    let app = Arc::clone(&state);
    blocking(move || {
        app.system.open_in_regedit(&key).map_err(|error| {
            CommandError::from(format!("Registry Editor could not be opened: {error}"))
        })
    })
    .await
}

#[tauri::command]
pub async fn reg_restart_explorer(state: App<'_>) -> CommandResult<()> {
    let app = Arc::clone(&state);
    blocking(move || {
        app.system.restart_explorer().map_err(|error| {
            CommandError::from(format!("File Explorer could not be restarted: {error}"))
        })
    })
    .await
}

/// Sign out of Windows, for tweaks that take effect at the next sign-in.
#[tauri::command]
pub async fn reg_sign_out(state: App<'_>) -> CommandResult<()> {
    let app = Arc::clone(&state);
    blocking(move || {
        app.system
            .sign_out()
            .map_err(|error| CommandError::from(format!("Windows did not sign out: {error}")))
    })
    .await
}

/// Restart Windows, for tweaks that take effect at the next start.
#[tauri::command]
pub async fn reg_restart_pc(state: App<'_>) -> CommandResult<()> {
    let app = Arc::clone(&state);
    blocking(move || {
        app.system
            .restart_pc()
            .map_err(|error| CommandError::from(format!("Windows did not restart: {error}")))
    })
    .await
}

/// Write an export the person placed with the Save dialog; `None` when they
/// cancelled it.
pub fn save_export(
    app: &AppHandle,
    data_dir: &Path,
    name: &str,
    filter: (&str, &[&str]),
    bytes: &[u8],
) -> CommandResult<Option<String>> {
    let Some(path) = crate::registry::platform::save_path(app, data_dir, name, &[filter]) else {
        return Ok(None);
    };
    write_export(&path, bytes)
}

/// Save a list as CSV, an Excel workbook or text, by the file type the
/// person chose in the Save dialog; `None` when they cancelled it.
pub fn save_table(
    app: &AppHandle,
    data_dir: &Path,
    stem: &str,
    sheet: &str,
    table: &wrc_core::export::Table,
) -> CommandResult<Option<String>> {
    let filters: [(&str, &[&str]); 3] = [
        ("Comma-separated values", &["csv"]),
        ("Excel workbook", &["xlsx"]),
        ("Text", &["txt"]),
    ];
    let Some(path) =
        crate::registry::platform::save_path(app, data_dir, &format!("{stem}.csv"), &filters)
    else {
        return Ok(None);
    };
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("xlsx") => write_export(&path, &table.xlsx(sheet)),
        Some("txt") => write_export(&path, table.text().as_bytes()),
        _ => write_export(&path, table.csv().as_bytes()),
    }
}

fn write_export(path: &Path, bytes: &[u8]) -> CommandResult<Option<String>> {
    std::fs::write(path, bytes).map_err(|error| {
        CommandError::from(format!("Could not save {}: {error}", path.display()))
    })?;
    Ok(Some(path.display().to_string()))
}

/// Delete backups past their keeping time or over the size limit, off the
/// start-up path.
pub fn prune_backups(data_dir: &Path, settings: &Settings) {
    let backups = settings.backups_dir(data_dir);
    let (days, megabytes) = (settings.keep_backups_days, settings.keep_backups_mb);
    std::thread::spawn(move || {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        if let Err(error) = wrc_core::backup::prune(&backups, days, now) {
            eprintln!("Old backups could not be deleted: {error}");
        }
        if let Err(error) = wrc_core::backup::trim(&backups, megabytes) {
            eprintln!("Backups over the size limit could not be deleted: {error}");
        }
    });
}
