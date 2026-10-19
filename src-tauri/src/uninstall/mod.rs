//! The page's commands. Each validates its input, hands the work to the
//! engine off the async runtime, and returns a typed result or a sentence.

pub mod platform;
pub mod prepare;
pub mod session;
pub mod state;
pub mod traced;

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use wu_core::Program;
use wu_core::backup::{self, RestoreReport, Summary};
use wu_core::folders::Folders;
use wu_core::history::{self, Entry};
use wu_core::identity::Identity;
use wu_core::programs::{find_program, forced_target, list_programs};
use wu_core::scan::{Mode, ScanReport};
use wu_core::settings::Settings;

use crate::error::{CommandResult, blocking};
use crate::uninstall::prepare::{attach_log, forced_folder};
use crate::uninstall::session::{RemovalView, Session, SessionView, now};
use crate::uninstall::state::AppState;

type App<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
pub fn un_get_settings(state: App<'_>) -> CommandResult<Settings> {
    Ok(Settings::load(&state.data_dir)?)
}

#[tauri::command]
pub fn un_save_settings(state: App<'_>, settings: Settings) -> CommandResult<()> {
    Ok(settings.save(&state.data_dir)?)
}

#[tauri::command]
pub async fn un_list_installed(state: App<'_>) -> CommandResult<Vec<Program>> {
    let app = Arc::clone(&state);
    blocking(move || {
        // A damaged settings file was reported at start; the list still
        // shows, with the defaults the page said it would use.
        let settings = Settings::load(&app.data_dir).unwrap_or_default();
        Ok(list_programs(
            app.system.as_ref(),
            settings.show_system_components,
        ))
    })
    .await
}

/// Open an uninstall of a listed program, recording what it is known by
/// (its executables included) before its uninstaller can delete them.
#[tauri::command]
pub async fn un_begin_uninstall(state: App<'_>, program_id: String) -> CommandResult<SessionView> {
    let app = Arc::clone(&state);
    blocking(move || {
        let system = app.system.as_ref();
        let program = find_program(system, &program_id)
            .ok_or("That program is no longer installed; refresh the list.")?;
        let folders = Folders::load(system)?;
        let mut identity = Identity::of_program(system, &folders, &program);
        let log = attach_log(&app.data_dir, &folders, &program, &mut identity);
        let session = app.add_session(|id| Session::new(id, Some(program), identity, false, log));
        Ok(session.view(system))
    })
    .await
}

/// Open a forced uninstall: no uninstaller, a scan by name and folder.
#[tauri::command]
pub async fn un_begin_forced(
    state: App<'_>,
    name: String,
    folder: String,
) -> CommandResult<SessionView> {
    let name = name.trim().to_owned();
    if name.chars().count() < 3 || name.chars().count() > 200 {
        return Err("Give the program's name (3 to 200 characters).".into());
    }
    let app = Arc::clone(&state);
    blocking(move || {
        let system = app.system.as_ref();
        let folders = Folders::load(system)?;
        let folder = forced_folder(&folder, &folders)?;
        let programs = list_programs(system, true);
        // A program still listed is the target itself: its entry and folder
        // are what is being removed, not another program's to protect.
        let listed = forced_target(system, &folders, &programs, &name, folder.as_deref()).cloned();
        let mut identity = match &listed {
            Some(program) => Identity::of_entry(system, &folders, program),
            None => Identity::named(&name, None, Vec::new()),
        };
        if let Some(folder) = folder.filter(|dir| !identity.owns_path(dir)) {
            identity.install_dirs.push(folder);
        }
        identity.read_executables();
        let log = listed
            .as_ref()
            .and_then(|program| attach_log(&app.data_dir, &folders, program, &mut identity));
        let session = app.add_session(|id| Session::new(id, listed, identity, true, log));
        Ok(session.view(app.system.as_ref()))
    })
    .await
}

#[tauri::command]
pub async fn un_create_restore_point(
    state: App<'_>,
    session: u32,
    batch: u32,
) -> CommandResult<()> {
    let app = Arc::clone(&state);
    let session = state.session(session)?;
    blocking(move || session.create_restore_point(app.system.as_ref(), batch)).await
}

/// Start the program's own uninstaller; returns the command that ran.
#[tauri::command]
pub async fn un_run_uninstaller(
    state: App<'_>,
    session: u32,
    quiet: bool,
) -> CommandResult<String> {
    let app = Arc::clone(&state);
    let session = state.session(session)?;
    blocking(move || session.run_uninstaller(app.system.as_ref(), quiet)).await
}

#[tauri::command]
pub fn un_uninstaller_finished(state: App<'_>, session: u32) -> CommandResult<bool> {
    state.session(session)?.uninstaller_finished()
}

#[tauri::command]
pub async fn un_scan_leftovers(
    state: App<'_>,
    session: u32,
    mode: Mode,
) -> CommandResult<ScanReport> {
    let app = Arc::clone(&state);
    let session = state.session(session)?;
    blocking(move || session.scan(&app, mode)).await
}

#[tauri::command]
pub fn un_scan_progress(state: App<'_>, session: u32) -> CommandResult<String> {
    Ok(state.session(session)?.progress.current())
}

#[tauri::command]
pub fn un_cancel_scan(state: App<'_>, session: u32) -> CommandResult<()> {
    state.session(session)?.progress.cancel();
    Ok(())
}

#[tauri::command]
pub async fn un_remove_leftovers(
    state: App<'_>,
    session: u32,
    ids: Vec<u32>,
) -> CommandResult<RemovalView> {
    if ids.is_empty() {
        return Err("Tick at least one leftover to remove.".into());
    }
    let app = Arc::clone(&state);
    let session = state.session(session)?;
    blocking(move || {
        let _removing = app.begin_removal();
        session.remove(&app, &ids)
    })
    .await
}

/// Close an uninstall, recording it if its uninstaller ran.
#[tauri::command]
pub fn un_end_uninstall(state: App<'_>, session: u32) -> CommandResult<()> {
    if let Some(session) = state.end_session(session) {
        session.record(&state, None, None)?;
    }
    Ok(())
}

#[tauri::command]
pub fn un_list_backups(state: App<'_>) -> Vec<Summary> {
    backup::list(&state.backups_dir())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreView {
    restored: u32,
    failed: Vec<crate::uninstall::session::FailedItem>,
}

#[tauri::command]
pub async fn un_restore_backup(state: App<'_>, id: String) -> CommandResult<RestoreView> {
    let app = Arc::clone(&state);
    blocking(move || {
        let _removing = app.begin_removal();
        let restored = backup::restore(app.system.as_ref(), &app.backups_dir(), &id, now());
        crate::uninstall::session::release_hives(app.system.as_ref());
        let RestoreReport { restored, failed } = restored?;
        let failed = failed
            .into_iter()
            .map(|(item, error)| crate::uninstall::session::FailedItem { item, error })
            .collect();
        Ok(RestoreView { restored, failed })
    })
    .await
}

#[tauri::command]
pub async fn un_delete_backup(state: App<'_>, id: String) -> CommandResult<()> {
    let app = Arc::clone(&state);
    blocking(move || Ok(backup::delete(&app.backups_dir(), &id)?)).await
}

#[tauri::command]
pub fn un_list_history(state: App<'_>) -> CommandResult<Vec<Entry>> {
    Ok(history::load(&state.data_dir)?)
}

#[tauri::command]
pub fn un_clear_history(state: App<'_>) -> CommandResult<()> {
    Ok(history::clear(&state.data_dir)?)
}
