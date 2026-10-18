//! The cleaner list, preview, clean and stop.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use wcc_core::catalog::{self, CleanerView, WINAPP2_FILE};
use wcc_core::history::{self, Activity, Entry};
use wcc_core::model::OptionKey;
use wcc_core::report::{Mode, Progress, Report, Sink};

use super::{App, record};
use crate::disk::state::AppState;
use crate::error::{CommandError, CommandResult, blocking};

/// The window repaints progress at most this often.
pub const PROGRESS_EVERY: Duration = Duration::from_millis(80);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPayload {
    cleaners: Vec<CleanerView>,
    problems: Vec<String>,
    /// The selection to show: saved, or the defaults before the first change.
    selection: Vec<String>,
    community: CommunityStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityStatus {
    /// A winapp2.ini has been imported.
    pub present: bool,
    /// Cleaners it added (zero while switched off).
    pub cleaners: usize,
    pub enabled: bool,
}

pub fn community_status(state: &AppState) -> CommunityStatus {
    CommunityStatus {
        present: state.cleaners_dir().join(WINAPP2_FILE).is_file(),
        cleaners: state
            .catalog()
            .cleaners
            .iter()
            .filter(|c| c.source == wcc_core::model::Source::Community)
            .count(),
        enabled: state.settings().community,
    }
}

/// Every cleaner with whether it applies to this PC and whether its program
/// is running.
#[tauri::command]
pub async fn disk_get_catalog(state: App<'_>) -> CommandResult<CatalogPayload> {
    let app = Arc::clone(&state);
    blocking(move || {
        let catalog = app.catalog();
        let cleaners = catalog::views(&catalog, app.system.as_ref());
        let selection = app
            .settings()
            .selected(&catalog.defaults())
            .iter()
            .filter(|key| catalog.contains(key))
            .map(ToString::to_string)
            .collect();
        let community = community_status(&app);
        Ok(CatalogPayload {
            cleaners,
            problems: catalog.problems.clone(),
            selection,
            community,
        })
    })
    .await
}

/// Sends progress to the page now and then, and stops when asked.
pub struct ChannelSink<'a, T: Serialize + Clone> {
    pub channel: &'a Channel<T>,
    pub state: &'a AppState,
    pub last: Mutex<Option<Instant>>,
}

impl<'a, T: Serialize + Clone> ChannelSink<'a, T> {
    pub fn new(channel: &'a Channel<T>, state: &'a AppState) -> Self {
        Self {
            channel,
            state,
            last: Mutex::new(None),
        }
    }

    /// Sends when enough time has passed, or always when `force`.
    pub fn send(&self, value: &T, force: bool) {
        let Ok(mut last) = self.last.lock() else {
            return;
        };
        let due = last.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY);
        if force || due {
            *last = Some(Instant::now());
            // A closed page cannot take progress; the job carries on.
            let _sent = self.channel.send(value.clone());
        }
    }
}

impl Sink for ChannelSink<'_, Progress> {
    fn progress(&self, progress: &Progress) {
        self.send(progress, progress.label.is_empty());
    }

    fn cancelled(&self) -> bool {
        self.state.cancelled()
    }
}

/// Checks the page's selection against the catalog.
fn selection(state: &AppState, keys: &[String]) -> CommandResult<Vec<OptionKey>> {
    if keys.len() > 50_000 {
        return Err("Too many options were sent.".into());
    }
    let catalog = state.catalog();
    let chosen: Vec<OptionKey> = keys
        .iter()
        .filter_map(|k| OptionKey::parse(k))
        .filter(|k| catalog.contains(k))
        .collect();
    if chosen.is_empty() {
        return Err("Tick at least one option first.".into());
    }
    Ok(chosen)
}

/// A preview (lists what would go) or a clean (removes it), with progress
/// sent to `on_progress`.
#[tauri::command]
pub async fn disk_run_cleaner(
    state: App<'_>,
    mode: Mode,
    selection: Vec<String>,
    on_progress: Channel<Progress>,
) -> CommandResult<Report> {
    let app = Arc::clone(&state);
    blocking(move || {
        let chosen = self::selection(&app, &selection)?;
        let _job = app.start_job()?;
        let catalog = app.catalog();
        let settings = app.run_settings();
        let sink = ChannelSink::new(&on_progress, &app);
        let report = wcc_core::run::run(
            &catalog,
            &chosen,
            mode,
            app.system.as_ref(),
            &settings,
            &sink,
        );
        if mode == Mode::Clean {
            record(&app, history_entry(&report));
        }
        Ok(report)
    })
    .await
}

fn history_entry(report: &Report) -> Entry {
    let mut names: Vec<&str> = report.options.iter().map(|o| o.cleaner.as_str()).collect();
    names.dedup();
    let summary = match names.len() {
        0 => "Nothing".to_string(),
        1..=3 => names.join(", "),
        n => format!("{}, {} and {} more", names[0], names[1], n - 2),
    };
    Entry {
        at: history::now(),
        activity: Activity::Clean,
        summary,
        items: report.totals.items(),
        bytes: report.totals.bytes,
        problems: report.totals.problems,
        cancelled: report.cancelled,
    }
}

/// Asks the running job to stop at its next step.
#[tauri::command]
pub fn disk_cancel_job(state: App<'_>) {
    state.cancel();
}

/// Reads custom cleaner files and winapp2.ini again.
#[tauri::command]
pub async fn disk_reload_catalog(state: App<'_>) -> CommandResult<()> {
    let app = Arc::clone(&state);
    blocking(move || {
        if app.is_busy() {
            return Err(CommandError::from(
                "Wait for the running job to finish first.",
            ));
        }
        app.reload_catalog();
        Ok(())
    })
    .await
}
