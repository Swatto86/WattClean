//! The Registry Tuner's commands.

use std::sync::Arc;

use serde::Serialize;
use wrc_core::tuner::{self, TweakView};

use super::App;
use crate::error::{CommandResult, blocking};

#[tauri::command]
pub async fn reg_list_tweaks(state: App<'_>) -> CommandResult<Vec<TweakView>> {
    let app = Arc::clone(&state);
    blocking(move || Ok(tuner::list(app.system.as_ref(), &app.data_dir)?)).await
}

#[tauri::command]
pub async fn reg_set_tweak(state: App<'_>, id: String, on: bool) -> CommandResult<TweakView> {
    let app = Arc::clone(&state);
    blocking(move || Ok(tuner::set(app.system.as_ref(), &app.data_dir, &id, on)?)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetOutcome {
    reset: usize,
    failed: Vec<(String, String)>,
}

/// Put back every tweak the app switched on.
#[tauri::command]
pub async fn reg_reset_tweaks(state: App<'_>) -> CommandResult<ResetOutcome> {
    let app = Arc::clone(&state);
    blocking(move || {
        let (reset, failed) = tuner::reset_all(app.system.as_ref(), &app.data_dir)?;
        Ok(ResetOutcome { reset, failed })
    })
    .await
}
