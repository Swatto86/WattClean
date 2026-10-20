//! Shredding chosen files and folders, and wiping a drive's free space.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::AppHandle;
use tauri::ipc::Channel;
use wcc_core::history::{self, Activity, Entry};
use wcc_core::paths::Guard;
use wcc_core::settings::check_full_path;
use wcc_core::shred::{self, ShredReport, WipeReport};
use wcc_core::system::Drive;

use super::cleaner::ChannelSink;
use super::{App, record};
use crate::disk::platform::{self, Pick};
use crate::error::{CommandError, CommandResult, blocking};

/// Files, or one folder, to add to the shred list; empty when cancelled.
#[tauri::command]
pub async fn disk_pick_to_shred(
    app: AppHandle,
    state: App<'_>,
    folder: bool,
) -> CommandResult<Vec<String>> {
    let data_dir = state.data_dir.clone();
    blocking(move || {
        let (what, title) = if folder {
            (Pick::Folder, "Choose a folder to shred")
        } else {
            (Pick::Files, "Choose files to shred")
        };
        Ok(platform::pick(&app, &data_dir, what, title)
            .into_iter()
            .map(|p| p.display().to_string())
            .collect())
    })
    .await
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShredProgress {
    files: u64,
    folders: u64,
    bytes: u64,
    current: String,
}

/// Overwrites and deletes the files and folders. Protected folders and the
/// keep list are refused, as in a clean.
#[tauri::command]
pub async fn disk_shred(
    state: App<'_>,
    paths: Vec<String>,
    on_progress: Channel<ShredProgress>,
) -> CommandResult<ShredReport> {
    let app = Arc::clone(&state);
    blocking(move || {
        if paths.is_empty() || paths.len() > 10_000 {
            return Err(CommandError::from("Add files or folders to shred first."));
        }
        for path in &paths {
            check_full_path(path).map_err(CommandError::from)?;
        }
        let _job = app.start_job()?;
        let settings = app.run_settings();
        let guard = Guard::new(&app.system.protected_folders(), &settings.keep);
        let sink = ChannelSink::new(&on_progress, &app);
        let targets: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let report = shred::shred_paths(
            &targets,
            &guard,
            &|path| app.system.link_count(path),
            &|| app.cancelled(),
            &|report, current| {
                sink.send(
                    &ShredProgress {
                        files: report.files,
                        folders: report.folders,
                        bytes: report.bytes,
                        current: current.display().to_string(),
                    },
                    false,
                );
            },
        );
        record(
            &app,
            Entry {
                at: history::now(),
                activity: Activity::Shred,
                summary: if paths.len() == 1 {
                    paths[0].clone()
                } else {
                    format!("{} files and folders", paths.len())
                },
                items: report.files + report.folders,
                bytes: report.bytes,
                problems: u64::try_from(report.problems.len()).unwrap_or(u64::MAX),
                cancelled: report.cancelled,
            },
        );
        Ok(report)
    })
    .await
}

/// The drives a free-space wipe may fill: never one that compresses what
/// is written.
#[tauri::command]
pub async fn disk_get_drives(state: App<'_>) -> CommandResult<Vec<Drive>> {
    let app = Arc::clone(&state);
    blocking(move || {
        Ok(app
            .system
            .drives()
            .into_iter()
            .filter(|drive| !app.system.compresses(drive))
            .collect())
    })
    .await
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WipeProgress {
    written: u64,
    target: u64,
}

/// Fills the drive's free space with zeros and gives it back.
#[tauri::command]
pub async fn disk_wipe_free_space(
    state: App<'_>,
    root: String,
    on_progress: Channel<WipeProgress>,
) -> CommandResult<WipeReport> {
    let app = Arc::clone(&state);
    blocking(move || {
        let drive = app
            .system
            .drives()
            .into_iter()
            // Linux's mount points differ by letter case; Windows' and
            // macOS's drive names do not.
            .find(|d| {
                if cfg!(target_os = "linux") {
                    d.root == root
                } else {
                    d.root.eq_ignore_ascii_case(&root)
                }
            })
            .ok_or_else(|| {
                CommandError::from(format!("{root} is not a drive WattClean can wipe."))
            })?;
        if app.system.compresses(&drive) {
            return Err(CommandError::from(format!(
                "{root} is not wiped: {}.",
                wcc_core::system::COMPRESSES
            )));
        }
        let _job = app.start_job()?;
        let folder = app.system.wipe_folder(&drive);
        let limit = app.system.wipe_limit();
        let target = limit.unwrap_or_else(|| app.system.free_space(&folder).unwrap_or(drive.free));
        let sink = ChannelSink::new(&on_progress, &app);
        let report = shred::wipe_free_space(&folder, limit, &|| app.cancelled(), &|written| {
            sink.send(&WipeProgress { written, target }, false);
        })
        .map_err(|error| CommandError::from(format!("Wiping {} stopped: {error}", drive.root)))?;
        record(
            &app,
            Entry {
                at: history::now(),
                activity: Activity::Wipe,
                summary: format!("Free space on {}", drive.root),
                items: 0,
                bytes: report.written,
                problems: 0,
                cancelled: report.cancelled,
            },
        );
        Ok(report)
    })
    .await
}
