//! winapp2.ini: importing a copy, downloading the community's latest (only
//! when the person clicks), and removing it.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use tauri::AppHandle;
use wcc_core::catalog::WINAPP2_FILE;
use wcc_core::winapp2;

use super::App;
use super::cleaner::{CommunityStatus, community_status};
use crate::disk::platform::{self, Pick};
use crate::disk::state::AppState;
use crate::error::{CommandError, CommandResult, blocking};

/// winapp2.ini is about 2 MB; anything far larger is not it.
const MAX_SIZE: u64 = 32 << 20;

/// Checks a candidate file and moves it into place as the app's copy.
fn install(
    state: &AppState,
    candidate: &Path,
    remove_candidate: bool,
) -> CommandResult<CommunityStatus> {
    let size = fs::metadata(candidate)?.len();
    if size > MAX_SIZE {
        return Err("That file is too large to be winapp2.ini.".into());
    }
    let text = String::from_utf8_lossy(&fs::read(candidate)?).into_owned();
    let entries = winapp2::parse(&text).len();
    if entries == 0 {
        return Err("No cleaner entries were found in that file, so it was not imported.".into());
    }
    let dir = state.cleaners_dir();
    fs::create_dir_all(&dir)?;
    let target = dir.join(WINAPP2_FILE);
    let temp = dir.join("winapp2.ini.tmp");
    fs::write(&temp, text.as_bytes())?;
    fs::rename(&temp, &target)?;
    if remove_candidate {
        let _removed = fs::remove_file(candidate);
    }
    state.update_settings(|settings| {
        settings.community = true;
        Ok(())
    })?;
    state.reload_catalog();
    Ok(community_status(state))
}

/// Imports a winapp2.ini chosen in an Open dialog; `None` when cancelled.
#[tauri::command]
pub async fn disk_import_winapp2(
    app: AppHandle,
    state: App<'_>,
) -> CommandResult<Option<CommunityStatus>> {
    let shared = Arc::clone(&state);
    blocking(move || {
        let Some(file) = platform::pick(&app, &shared.data_dir, Pick::Ini, "Import winapp2.ini")
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        if shared.is_busy() {
            return Err(CommandError::from(
                "Wait for the running job to finish first.",
            ));
        }
        install(&shared, &file, false).map(Some)
    })
    .await
}

/// Downloads the community's current winapp2.ini over HTTPS and imports it.
#[tauri::command]
pub async fn disk_download_winapp2(state: App<'_>) -> CommandResult<CommunityStatus> {
    let shared = Arc::clone(&state);
    blocking(move || {
        if shared.fake {
            return Err(CommandError::from("The test machine does not go online."));
        }
        if shared.is_busy() {
            return Err(CommandError::from(
                "Wait for the running job to finish first.",
            ));
        }
        let dir = shared.cleaners_dir();
        fs::create_dir_all(&dir)?;
        let download = dir.join("winapp2.download");
        platform::download(platform::WINAPP2_URL, &download)?;
        install(&shared, &download, true)
    })
    .await
}

#[tauri::command]
pub async fn disk_remove_winapp2(state: App<'_>) -> CommandResult<CommunityStatus> {
    let shared = Arc::clone(&state);
    blocking(move || {
        if shared.is_busy() {
            return Err(CommandError::from(
                "Wait for the running job to finish first.",
            ));
        }
        let file = shared.cleaners_dir().join(WINAPP2_FILE);
        if file.exists() {
            fs::remove_file(&file)?;
        }
        shared.reload_catalog();
        Ok(community_status(&shared))
    })
    .await
}
