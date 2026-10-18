//! Background running: scheduled jobs, starting with Windows, keeping the
//! app in the notification area, and quitting.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use wrc_core::autostart;
use wrc_core::jobs::{self, Job, JobInput, LastRun};
use wrc_core::launch::not_installed;

use super::App;
use crate::error::{CommandError, CommandResult, blocking};
use crate::registry::state::AppState;

/// This executable, when it is an installed copy Windows may start later;
/// otherwise why not.
pub fn launcher() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("this program cannot be found: {error}"))?;
    let downloads =
        std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Downloads"));
    match not_installed(&exe, &std::env::temp_dir(), downloads.as_deref()) {
        Some(reason) => Err(reason),
        None => Ok(exe),
    }
}

#[tauri::command]
pub async fn reg_list_jobs(state: App<'_>) -> CommandResult<Vec<Job>> {
    let app = Arc::clone(&state);
    blocking(move || Ok(jobs::list(&app.data_dir)?)).await
}

#[tauri::command]
pub async fn reg_save_job(state: App<'_>, job: JobInput) -> CommandResult<Job> {
    let app = Arc::clone(&state);
    blocking(move || {
        let exe = launcher();
        Ok(jobs::save_job(
            app.system.as_ref(),
            &app.data_dir,
            job,
            exe.as_deref().map_err(Clone::clone),
        )?)
    })
    .await
}

#[tauri::command]
pub async fn reg_set_job_enabled(state: App<'_>, id: String, enabled: bool) -> CommandResult<Job> {
    let app = Arc::clone(&state);
    blocking(move || {
        let exe = launcher();
        Ok(jobs::set_enabled(
            app.system.as_ref(),
            &app.data_dir,
            &id,
            enabled,
            exe.as_deref().map_err(Clone::clone),
        )?)
    })
    .await
}

#[tauri::command]
pub async fn reg_delete_job(state: App<'_>, id: String) -> CommandResult<()> {
    let app = Arc::clone(&state);
    blocking(move || Ok(jobs::delete(app.system.as_ref(), &app.data_dir, &id)?)).await
}

/// Run a job now, in the app, as its task would.
#[tauri::command]
pub async fn reg_run_job_now(state: App<'_>, id: String) -> CommandResult<LastRun> {
    let app = Arc::clone(&state);
    let _scan = app.begin_scan()?;
    let result = blocking({
        let app = Arc::clone(&app);
        move || Ok(jobs::run(app.system.as_ref(), &app.data_dir, &id)?)
    })
    .await;
    app.end_scan();
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Background {
    /// Windows starts the app at sign-in.
    start_with_windows: bool,
    /// ...with administrator rights, through the sign-in task.
    starts_elevated: bool,
    /// Why starting with Windows cannot be switched on from this copy.
    start_problem: Option<String>,
}

fn background(app: &AppState) -> Background {
    let start = autostart::state(app.system.as_ref());
    Background {
        start_with_windows: start.on,
        starts_elevated: start.elevated,
        start_problem: launcher().err(),
    }
}

#[tauri::command]
pub fn reg_background_state(state: App<'_>) -> Background {
    background(&state)
}

/// Start (or stop starting) with Windows, in the notification area; with
/// administrator rights when switched on as administrator.
#[tauri::command]
pub async fn reg_set_start_with_windows(state: App<'_>, on: bool) -> CommandResult<Background> {
    let app = Arc::clone(&state);
    blocking(move || {
        if on {
            let exe = launcher()
                .map_err(|reason| CommandError::from(format!("Not switched on: {reason}")))?;
            autostart::enable(app.system.as_ref(), &exe)?;
        } else {
            autostart::disable(app.system.as_ref())?;
        }
        Ok(background(&app))
    })
    .await
}
