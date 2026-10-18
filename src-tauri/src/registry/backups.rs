//! The Backup Center: list, inspect, restore, export and delete backups,
//! and back up a key by hand.

use std::sync::Arc;

use serde::Serialize;
use tauri::AppHandle;
use wrc_core::backup::{
    self, ItemState, RestoreOutcome, Summary, describe_target, reg_file_bytes, to_reg_file,
};

use super::{App, key_from_page, save_export};
use crate::error::{CommandResult, blocking};

#[tauri::command]
pub async fn reg_list_backups(state: App<'_>) -> CommandResult<Vec<Summary>> {
    let dir = state.backups_dir();
    blocking(move || Ok(backup::list(&dir)?)).await
}

/// One entry of a backup, for choosing what to restore.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    index: usize,
    entry: String,
    detail: String,
    category: Option<String>,
    /// `removed`, `kept`, `restored`, `failed` or `pending`.
    state: &'static str,
    reason: Option<String>,
    restorable: bool,
}

#[tauri::command]
pub async fn reg_backup_items(state: App<'_>, id: String) -> CommandResult<Vec<ItemView>> {
    let dir = state.backups_dir();
    blocking(move || {
        let backup = backup::load(&dir, &id)?;
        Ok(backup
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let (state, reason) = match &item.state {
                    ItemState::Pending => ("pending", None),
                    ItemState::Removed => ("removed", None),
                    ItemState::Kept => ("kept", None),
                    ItemState::Restored => ("restored", None),
                    ItemState::Failed { reason } => ("failed", Some(reason.clone())),
                };
                ItemView {
                    index,
                    entry: describe_target(&item.key, item.value.as_deref()),
                    detail: item.detail.clone(),
                    category: item.category.map(|category| category.title().to_owned()),
                    state,
                    reason,
                    restorable: item.saved.is_some()
                        && matches!(state, "pending" | "removed" | "kept"),
                }
            })
            .collect())
    })
    .await
}

/// Save the whole registry's hives, as administrator.
#[tauri::command]
pub async fn reg_back_up_full(state: App<'_>) -> CommandResult<Summary> {
    let app = Arc::clone(&state);
    blocking(move || {
        Ok(backup::back_up_full(
            app.system.as_ref(),
            &app.backups_dir(),
            "",
        )?)
    })
    .await
}

/// Restore the chosen items (by index), or all when none are named.
#[tauri::command]
pub async fn reg_restore_backup(
    state: App<'_>,
    id: String,
    items: Option<Vec<usize>>,
) -> CommandResult<RestoreOutcome> {
    let app = Arc::clone(&state);
    blocking(move || {
        Ok(backup::restore(
            app.system.as_ref(),
            &app.backups_dir(),
            &id,
            items.as_deref(),
        )?)
    })
    .await
}

#[tauri::command]
pub async fn reg_delete_backup(state: App<'_>, id: String) -> CommandResult<()> {
    let dir = state.backups_dir();
    blocking(move || Ok(backup::delete(&dir, &id)?)).await
}

/// Delete every backup; returns how many there were.
#[tauri::command]
pub async fn reg_delete_all_backups(state: App<'_>) -> CommandResult<usize> {
    let dir = state.backups_dir();
    blocking(move || {
        let all = backup::list(&dir)?;
        for summary in &all {
            backup::delete(&dir, &summary.id)?;
        }
        Ok(all.len())
    })
    .await
}

/// Save a backup as a `.reg` file Registry Editor can merge.
#[tauri::command]
pub async fn reg_export_backup(
    app: AppHandle,
    state: App<'_>,
    id: String,
) -> CommandResult<Option<String>> {
    let dir = state.backups_dir();
    let data_dir = state.data_dir.clone();
    blocking(move || {
        let backup = backup::load(&dir, &id)?;
        let bytes = reg_file_bytes(&to_reg_file(&backup));
        let name = format!("WattClean backup {id}.reg");
        save_export(
            &app,
            &data_dir,
            &name,
            ("Registration entries", &["reg"]),
            &bytes,
        )
    })
    .await
}

/// Back up a key and everything beneath it; nothing is removed.
#[tauri::command]
pub async fn reg_back_up_key(state: App<'_>, key: String, label: String) -> CommandResult<Summary> {
    let key = key_from_page(&key)?;
    if key.path.is_empty() {
        return Err(
            "Back up a key inside the hive; a whole hive belongs in a System Restore point.".into(),
        );
    }
    let app = Arc::clone(&state);
    blocking(move || {
        let summary = backup::back_up_key(app.system.as_ref(), &app.backups_dir(), &key, &label)?;
        let _ = app.system.release_account_hives();
        Ok(summary)
    })
    .await
}
