//! The page's commands. Each validates its input, hands the work to the
//! engine off the async runtime, and returns a typed result or a sentence.

pub mod cleaner;
pub mod community;
pub mod platform;
pub mod state;
pub mod tools;

use std::sync::Arc;

use tauri::{AppHandle, State};
use wcc_core::history::{self, Entry};
use wcc_core::settings::{Settings, SettingsPatch};

use crate::disk::platform::Pick;
use crate::disk::state::AppState;
use crate::error::{CommandError, CommandResult, blocking};

pub type App<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
pub fn disk_get_settings(state: App<'_>) -> Settings {
    state.settings()
}

/// Applies a change the page made, saves it, and returns the settings as
/// they now are. Turning the community cleaners on or off reloads the list.
#[tauri::command]
pub async fn disk_update_settings(state: App<'_>, patch: SettingsPatch) -> CommandResult<Settings> {
    let app = Arc::clone(&state);
    blocking(move || {
        let (before, settings) =
            app.update_settings(|settings| settings.apply(patch).map_err(CommandError::from))?;
        if settings.community != before.community {
            app.reload_catalog();
        }
        Ok(settings)
    })
    .await
}

/// A file or folder for the keep list or Custom paths, from an Open dialog;
/// `None` when the dialog was cancelled.
#[tauri::command]
pub async fn disk_pick_path(
    app: AppHandle,
    state: App<'_>,
    folder: bool,
) -> CommandResult<Option<String>> {
    let data_dir = state.data_dir.clone();
    blocking(move || {
        let (what, title) = if folder {
            (Pick::Folder, "Choose a folder")
        } else {
            (Pick::Files, "Choose a file")
        };
        Ok(platform::pick(&app, &data_dir, what, title)
            .into_iter()
            .next()
            .map(|p| p.display().to_string()))
    })
    .await
}

#[tauri::command]
pub async fn disk_get_history(state: App<'_>) -> CommandResult<Vec<Entry>> {
    let path = state.history_path();
    blocking(move || Ok(history::load(&path))).await
}

#[tauri::command]
pub async fn disk_clear_history(state: App<'_>) -> CommandResult<()> {
    let path = state.history_path();
    blocking(move || history::clear(&path).map_err(CommandError::from)).await
}

/// Records what a clean, shred or wipe did. A failure to record is
/// reported on stderr only: the work itself is done.
pub fn record(state: &AppState, entry: Entry) {
    if let Err(error) = history::append(&state.history_path(), entry) {
        eprintln!("The history could not be saved: {error}");
    }
}

#[tauri::command]
pub fn disk_open_cleaners_folder(state: App<'_>) -> CommandResult<()> {
    platform::open_folder(state.fake, &state.cleaners_dir())
}

/// A wipe cut short (power loss, a killed process) leaves its filler
/// folder behind; find and delete it so the space comes back.
pub fn remove_wipe_leftovers(state: &AppState) {
    for drive in state.system.drives() {
        let folder = state.system.wipe_folder(&drive);
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("WattClean-wipe-")
                && entry.path().is_dir()
                && let Err(error) = std::fs::remove_dir_all(entry.path())
            {
                eprintln!("A leftover wipe folder could not be removed: {error}");
            }
        }
    }
}
