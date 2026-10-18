//! Scanning, searching and cleaning.

use std::sync::Arc;

use serde::Serialize;
use tauri::AppHandle;
use wrc_core::backup::{BackupKind, Removal, Target, describe_target, remove};
use wrc_core::export::findings_table;
use wrc_core::scan::{Category, Mode, Report, Request, scan};
use wrc_core::search::{SearchRequest, search};
use wrc_core::settings::Settings;

use super::{App, save_table};
use crate::error::{CommandError, CommandResult, blocking};
use crate::registry::state::{AppState, List};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInfo {
    id: Category,
    title: &'static str,
    description: &'static str,
    /// The first scan mode that includes it; `None` for custom scans only.
    from_mode: Option<Mode>,
}

/// The categories a custom scan can choose, with what each looks for.
#[tauri::command]
pub fn reg_categories() -> Vec<CategoryInfo> {
    Category::CHOOSABLE
        .iter()
        .map(|category| CategoryInfo {
            id: *category,
            title: category.title(),
            description: category.description(),
            from_mode: [Mode::Safe, Mode::Moderate, Mode::Advanced]
                .into_iter()
                .find(|mode| Category::for_mode(*mode, &[]).contains(category)),
        })
        .collect()
}

#[tauri::command]
pub async fn reg_scan_registry(state: App<'_>, request: Request) -> CommandResult<Report> {
    let app = Arc::clone(&state);
    let progress = app.begin_scan()?;
    let result = blocking({
        let app = Arc::clone(&app);
        move || {
            let options = Settings::load(&app.data_dir)?.options();
            Ok(scan(app.system.as_ref(), &request, &options, &progress))
        }
    })
    .await;
    app.end_scan();
    let report = result?;
    app.set_report(List::Cleaner, report.clone());
    Ok(report)
}

#[tauri::command]
pub async fn reg_search_registry(state: App<'_>, request: SearchRequest) -> CommandResult<Report> {
    request.roots().map_err(CommandError::from)?;
    let app = Arc::clone(&state);
    let progress = app.begin_scan()?;
    let result = blocking({
        let app = Arc::clone(&app);
        move || {
            let options = Settings::load(&app.data_dir)?.options();
            search(app.system.as_ref(), &request, &options, &progress).map_err(CommandError::from)
        }
    })
    .await;
    app.end_scan();
    let report = result?;
    app.set_report(List::Search, report.clone());
    Ok(report)
}

/// What the running scan is doing, or nothing when none runs.
#[tauri::command]
pub fn reg_scan_progress(state: App<'_>) -> String {
    state
        .progress()
        .map(|progress| progress.phase())
        .unwrap_or_default()
}

#[tauri::command]
pub fn reg_cancel_scan(state: App<'_>) {
    if let Some(progress) = state.progress() {
        progress.cancel();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanOutcome {
    #[serde(flatten)]
    removal: Removal,
    /// The findings that are gone now.
    removed_ids: Vec<u32>,
    /// Why no restore point was made, when one was asked for and failed.
    restore_point_error: Option<String>,
    /// Why no full registry backup was made, when one was asked for.
    full_backup_error: Option<String>,
}

/// Remove the chosen findings of the last scan or search, backing each up
/// first. Machine-wide entries are refused without administrator rights.
#[tauri::command]
pub async fn reg_clean(
    state: App<'_>,
    list: List,
    ids: Vec<u32>,
    restore_point: bool,
    force: bool,
    full_backup: bool,
) -> CommandResult<CleanOutcome> {
    if ids.is_empty() {
        return Err("Tick at least one entry to remove.".into());
    }
    if (force || full_backup) && !state.system.is_elevated() {
        return Err(
            "Taking ownership and a full registry backup need administrator rights.".into(),
        );
    }
    let app = Arc::clone(&state);
    let choices = Choices {
        restore_point,
        force,
        full_backup,
    };
    blocking(move || clean_now(&app, list, &ids, choices)).await
}

/// What the person chose in the clean dialog.
struct Choices {
    restore_point: bool,
    force: bool,
    full_backup: bool,
}

fn clean_now(
    app: &AppState,
    list: List,
    ids: &[u32],
    choices: Choices,
) -> CommandResult<CleanOutcome> {
    let Choices {
        restore_point,
        force,
        full_backup,
    } = choices;
    let report = app.report(list)?;
    let elevated = app.system.is_elevated();
    let mut chosen = Vec::new();
    for id in ids {
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.id == *id)
            .ok_or("Some of those entries are no longer in the results; scan again.")?;
        if finding.needs_admin && !elevated {
            return Err("Some ticked entries need administrator rights. Untick them, or restart as administrator.".into());
        }
        chosen.push(finding.clone());
    }
    let full_backup_error = full_backup
        .then(|| {
            wrc_core::backup::back_up_full(
                app.system.as_ref(),
                &app.backups_dir(),
                "Full backup before cleaning",
            )
            .err()
            .map(|error| error.to_string())
        })
        .flatten();
    let restore_point_error = restore_point
        .then(|| {
            app.system
                .create_restore_point("WattClean: before cleaning the registry")
                .err()
                .map(|error| error.to_string())
        })
        .flatten();
    let targets = chosen
        .iter()
        .map(|finding| Target {
            key: finding.key.clone(),
            value: finding.value.clone(),
            category: Some(finding.category),
            detail: finding.detail.clone(),
        })
        .collect();
    let (kind, label) = match list {
        List::Cleaner => (
            BackupKind::Clean,
            format!("Registry clean, {}", entries(chosen.len())),
        ),
        List::Search => (
            BackupKind::Search,
            format!("Search results, {}", entries(chosen.len())),
        ),
    };
    let removal = remove(
        app.system.as_ref(),
        &app.backups_dir(),
        kind,
        &label,
        targets,
        force,
    )?;
    let removed_ids: Vec<u32> = chosen
        .iter()
        .filter(|finding| {
            let shown = describe_target(&finding.key, finding.value.as_deref());
            !removal.failed.iter().any(|(failed, _)| *failed == shown)
        })
        .map(|finding| finding.id)
        .collect();
    app.drop_findings(list, &removed_ids);
    Ok(CleanOutcome {
        removal,
        removed_ids,
        restore_point_error,
        full_backup_error,
    })
}

fn entries(count: usize) -> String {
    if count == 1 {
        "1 entry".to_owned()
    } else {
        format!("{count} entries")
    }
}

/// Save the last results as CSV or a workbook; `None` when the Save dialog
/// was cancelled.
#[tauri::command]
pub async fn reg_export_findings(
    app: AppHandle,
    state: App<'_>,
    list: List,
) -> CommandResult<Option<String>> {
    let report = state.report(list)?;
    let data_dir = state.data_dir.clone();
    let (stem, sheet) = match list {
        List::Cleaner => ("WattClean scan", "Scan"),
        List::Search => ("WattClean search", "Search"),
    };
    blocking(move || {
        save_table(
            &app,
            &data_dir,
            stem,
            sheet,
            &findings_table(&report.findings),
        )
    })
    .await
}
