//! Revo's remaining options: exporting and importing the settings, moving
//! the backups to another folder, and searching the web for an entry.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::AppHandle;
use wrc_core::backup::move_backups;
use wrc_core::settings::Settings;

use super::{App, save_export};
use crate::error::{CommandError, CommandResult, blocking};

/// A settings file larger than this is not one.
const MAX_SETTINGS: u64 = 1 << 20;

/// Save the settings as a file; `None` when the Save dialog was cancelled.
#[tauri::command]
pub async fn reg_export_settings(app: AppHandle, state: App<'_>) -> CommandResult<Option<String>> {
    let data_dir = state.data_dir.clone();
    blocking(move || {
        let settings = Settings::load(&data_dir)?;
        let text = serde_json::to_string_pretty(&settings)
            .map_err(|error| CommandError::from(error.to_string()))?;
        save_export(
            &app,
            &data_dir,
            "WattClean registry settings.json",
            ("WattClean registry settings", &["json"]),
            text.as_bytes(),
        )
    })
    .await
}

/// Take the settings from a file the person picks; `None` when they
/// cancelled. Everything is checked as a save is, and the backups folder
/// stays where it is: moving backups is its own step.
#[tauri::command]
pub async fn reg_import_settings(
    app: AppHandle,
    state: App<'_>,
) -> CommandResult<Option<Settings>> {
    let data_dir = state.data_dir.clone();
    blocking(move || {
        let Some(path) = crate::registry::platform::open_path(
            &app,
            &data_dir,
            "WattClean registry settings.json",
            ("WattClean registry settings", &["json"]),
        ) else {
            return Ok(None);
        };
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        if size > MAX_SETTINGS {
            return Err("That is not a WattClean registry settings file.".into());
        }
        let text = std::fs::read_to_string(&path).map_err(|error| {
            CommandError::from(format!("Could not read {}: {error}", path.display()))
        })?;
        let mut settings: Settings = serde_json::from_str(&text).map_err(|error| {
            CommandError::from(format!(
                "That is not a WattClean registry settings file: {error}"
            ))
        })?;
        settings.backups_folder = Settings::load(&data_dir)?.backups_folder;
        settings.save(&data_dir)?;
        Ok(Some(settings))
    })
    .await
}

/// Move every backup to a folder the person picks, and keep new ones there;
/// `None` when they cancelled.
#[tauri::command]
pub async fn reg_choose_backups_folder(
    app: AppHandle,
    state: App<'_>,
) -> CommandResult<Option<String>> {
    let shared = Arc::clone(&state);
    let _busy = shared.begin_scan()?;
    let data_dir = state.data_dir.clone();
    let result = blocking(move || {
        let Some(folder) = crate::registry::platform::pick_folder(&app, &data_dir) else {
            return Ok(None);
        };
        relocate(&data_dir, Some(folder)).map(Some)
    })
    .await;
    shared.end_scan();
    result
}

/// Move every backup back into the data folder.
#[tauri::command]
pub async fn reg_default_backups_folder(state: App<'_>) -> CommandResult<String> {
    let shared = Arc::clone(&state);
    let _busy = shared.begin_scan()?;
    let data_dir = state.data_dir.clone();
    let result = blocking(move || relocate(&data_dir, None)).await;
    shared.end_scan();
    result
}

fn relocate(data_dir: &std::path::Path, folder: Option<PathBuf>) -> CommandResult<String> {
    let mut settings = Settings::load(data_dir)?;
    let from = settings.backups_dir(data_dir);
    let to = folder.clone().unwrap_or_else(|| data_dir.join("backups"));
    move_backups(&from, &to)?;
    settings.backups_folder = folder.map(|folder| folder.display().to_string());
    settings.save(data_dir)?;
    Ok(to.display().to_string())
}

/// Search the web for an entry's name in the person's browser, as Revo's
/// "Search at Google for" does. Only on request: the app itself makes no
/// network connection.
#[tauri::command]
pub async fn reg_search_web(state: App<'_>, text: String) -> CommandResult<()> {
    let text = text.trim().to_owned();
    if text.is_empty() || text.chars().count() > 200 {
        return Err("Search for a name of up to 200 characters.".into());
    }
    let url = format!("https://www.google.com/search?q={}", encode(&text));
    let app = Arc::clone(&state);
    blocking(move || {
        app.system
            .open_url(&url)
            .map_err(|error| CommandError::from(format!("The browser did not open: {error}")))
    })
    .await
}

/// Percent-encoding for a query: unreserved characters stay, the rest of
/// the UTF-8 bytes are escaped.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn queries_are_percent_encoded() {
        assert_eq!(super::encode("Fabrikam Updater"), "Fabrikam%20Updater");
        assert_eq!(super::encode("a&b=c/é"), "a%26b%3Dc%2F%C3%A9");
    }
}
