//! The Registry Watcher's commands.

use std::sync::Arc;

use tauri::AppHandle;
use wrc_core::model::RegPath;
use wrc_core::trace::TraceState;
use wrc_core::watch::{Event, WatchView, table};

use super::{App, key_from_page, save_table};
use crate::error::{CommandResult, blocking};

#[tauri::command]
pub fn reg_list_watches(state: App<'_>) -> Vec<WatchView> {
    state.watcher.list()
}

/// Whether changes are logged with the process that made them.
#[tauri::command]
pub fn reg_watch_tracing(state: App<'_>) -> TraceState {
    state.watcher.tracing()
}

/// Start watching a key; returns it as the watch list names it.
#[tauri::command]
pub async fn reg_add_watch(state: App<'_>, key: String) -> CommandResult<RegPath> {
    let key = key_from_page(&key)?;
    if key.path.is_empty() {
        return Err("Watch a key inside the hive, not the whole hive.".into());
    }
    let app = Arc::clone(&state);
    blocking(move || {
        app.watcher.add(key.clone())?;
        Ok(key)
    })
    .await
}

#[tauri::command]
pub async fn reg_remove_watch(state: App<'_>, key: String) -> CommandResult<()> {
    let key = key_from_page(&key)?;
    let app = Arc::clone(&state);
    blocking(move || Ok(app.watcher.remove(&key)?)).await
}

#[tauri::command]
pub async fn reg_pause_watch(state: App<'_>, key: String, paused: bool) -> CommandResult<()> {
    let key = key_from_page(&key)?;
    let app = Arc::clone(&state);
    blocking(move || Ok(app.watcher.pause(&key, paused)?)).await
}

/// Changes logged for `key` after event number `since`.
#[tauri::command]
pub fn reg_watch_events(state: App<'_>, key: String, since: u64) -> CommandResult<Vec<Event>> {
    let key = key_from_page(&key)?;
    Ok(state.watcher.events(&key, since)?)
}

#[tauri::command]
pub fn reg_clear_watch(state: App<'_>, key: String) -> CommandResult<()> {
    let key = key_from_page(&key)?;
    Ok(state.watcher.clear(&key)?)
}

/// Save a key's log as CSV; `None` when the Save dialog was cancelled.
#[tauri::command]
pub async fn reg_export_watch(
    app: AppHandle,
    state: App<'_>,
    key: String,
) -> CommandResult<Option<String>> {
    let key = key_from_page(&key)?;
    let events = state.watcher.events(&key, 0)?;
    let data_dir = state.data_dir.clone();
    blocking(move || {
        save_table(
            &app,
            &data_dir,
            "WattClean watch log",
            "Watch log",
            &table(&events),
        )
    })
    .await
}
