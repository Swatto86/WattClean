//! Commands for installing with tracing and for the install logs it saves.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};
use wu_core::command_line::installer_launch;
use wu_core::folders::Folders;
use wu_core::install_log::{self, InstallLog, LogSummary};

use crate::error::{CommandResult, blocking};
use crate::uninstall::session::now;
use crate::uninstall::state::{AppState, TraceRun};

type App<'a> = State<'a, Arc<AppState>>;

/// Ask for an installer with the native file picker; `None` when cancelled.
#[tauri::command]
pub async fn un_pick_installer(app: AppHandle, state: App<'_>) -> CommandResult<Option<String>> {
    let data_dir = state.data_dir.clone();
    blocking(move || {
        Ok(crate::uninstall::platform::pick_installer(&app, &data_dir)
            .map(|path| path.display().to_string()))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceStarted {
    id: u32,
    installer: String,
}

/// Start the installer under an install trace.
#[tauri::command]
pub async fn un_begin_trace(state: App<'_>, path: String) -> CommandResult<TraceStarted> {
    let app = Arc::clone(&state);
    blocking(move || {
        let system = app.system.as_ref();
        if !system.is_elevated() {
            return Err("Tracing an install needs administrator rights. Restart WattClean as administrator first.".into());
        }
        let path = PathBuf::from(path.trim().trim_matches('"'));
        let folders = Folders::load(system)?;
        let launch = installer_launch(&folders, &path)?;
        let tracing = system
            .trace_install(&launch)
            .map_err(|error| format!("The installer did not start under tracing: {error}"))?;
        let installer = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = app.add_trace(TraceRun { tracing, installer: installer.clone() });
        Ok(TraceStarted { id, installer })
    })
    .await
}

#[tauri::command]
pub fn un_trace_finished(state: App<'_>, id: u32) -> CommandResult<bool> {
    state.trace_finished(id)
}

/// Stop tracing, save the install log and return its summary.
#[tauri::command]
pub async fn un_finish_trace(state: App<'_>, id: u32) -> CommandResult<LogSummary> {
    let app = Arc::clone(&state);
    let run = state.take_trace(id)?;
    blocking(move || {
        let trace = run
            .tracing
            .finish()
            .map_err(|error| format!("Tracing did not finish cleanly: {error}"))?;
        let system = app.system.as_ref();
        let folders = Folders::load(system)?;
        let mut log = install_log::build(system, &folders, &trace, &run.installer, now());
        install_log::save(&app.data_dir, &mut log)?;
        Ok(log.summary())
    })
    .await
}

#[tauri::command]
pub fn un_list_logs(state: App<'_>) -> Vec<LogSummary> {
    install_log::list(&state.data_dir)
        .iter()
        .map(InstallLog::summary)
        .collect()
}

#[tauri::command]
pub fn un_get_log(state: App<'_>, id: String) -> CommandResult<InstallLog> {
    Ok(install_log::load(&state.data_dir, &id)?)
}

#[tauri::command]
pub fn un_delete_log(state: App<'_>, id: String) -> CommandResult<()> {
    Ok(install_log::delete(&state.data_dir, &id)?)
}
