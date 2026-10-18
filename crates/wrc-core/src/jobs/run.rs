//! Running a job: what a scheduled task does when Windows starts it, and
//! what "Run now" does in the app.

use std::io;
use std::path::Path;

use super::{LastRun, Work, list, now, record, task_name};
use crate::backup::{BackupKind, Target, back_up_key, remove};
use crate::keyname::parse_key;
use crate::scan::{Progress, Request, scan};
use crate::settings::Settings;
use crate::system::System;
use crate::{Error, Result};

/// Run job `id` and record the outcome on it. A task whose job was deleted
/// removes itself. An Auto Clean removes only what the scan ticks (never
/// machine-wide entries without administrator rights) and never takes
/// ownership of protected keys: nobody is there to decide.
pub fn run(system: &dyn System, dir: &Path, id: &str) -> Result<LastRun> {
    let Some(job) = list(dir)?.into_iter().find(|job| job.id == id) else {
        let _ = system.delete_task(&task_name(id));
        return Err(Error::Invalid(format!(
            "Schedule {id} no longer exists; its task was removed."
        )));
    };
    let outcome = match &job.work {
        Work::Clean {
            mode,
            categories,
            all_accounts,
        } => {
            let request = Request {
                mode: *mode,
                categories: categories.clone(),
                all_accounts: *all_accounts,
            };
            let label = format!("Auto clean: {}", job.name);
            let point = format!("WattClean: {}", job.name);
            clean_ticked(system, dir, &request, &label, &point)
        }
        Work::Backup { key } => backup(system, dir, &job.name, key),
        Work::FullBackup => crate::backup::back_up_full(
            system,
            &crate::settings::backups_dir(dir),
            &format!("Auto full backup: {}", job.name),
        )
        .map(|summary| {
            (
                format!("Saved the whole registry ({} hives).", summary.items),
                Some(summary.id),
            )
        }),
    };
    let last = match outcome {
        Ok((summary, backup)) => LastRun {
            time: now(),
            ok: true,
            summary,
            backup,
        },
        Err(error) => LastRun {
            time: now(),
            ok: false,
            summary: error.to_string(),
            backup: None,
        },
    };
    record(dir, id, last.clone())?;
    if let Ok(settings) = Settings::load(dir) {
        let backups = settings.backups_dir(dir);
        let _ = crate::backup::prune(&backups, settings.keep_backups_days, now());
        let _ = crate::backup::trim(&backups, settings.keep_backups_mb);
    }
    Ok(last)
}

/// What Task Scheduler's start does: run job `id`, then say how it went in
/// a Windows notification, as no window is open to show it. A notification
/// Windows refuses is returned beside the outcome, never instead of it.
pub fn run_scheduled(
    system: &dyn System,
    dir: &Path,
    id: &str,
) -> (Result<LastRun>, io::Result<()>) {
    let name = list(dir)
        .ok()
        .and_then(|jobs| jobs.into_iter().find(|job| job.id == id))
        .map(|job| job.name);
    let outcome = run(system, dir, id);
    let (title, body) = notice(name.as_deref(), &outcome);
    let shown = system.notify(&title, &body);
    (outcome, shown)
}

/// The notification's title and text after a scheduled run.
pub fn notice(name: Option<&str>, outcome: &Result<LastRun>) -> (String, String) {
    match (name, outcome) {
        (Some(name), Ok(last)) if last.ok => (format!("\"{name}\" ran"), last.summary.clone()),
        (Some(name), Ok(last)) => (
            format!("\"{name}\" ran into a problem"),
            last.summary.clone(),
        ),
        (None, Ok(last)) => ("A schedule ran".into(), last.summary.clone()),
        (_, Err(error)) => ("A schedule could not run".into(), error.to_string()),
    }
}

/// Scan and remove what the scan ticks, with a backup labelled `label`
/// (and a restore point named `point` when the settings ask for one), as
/// nobody is there to choose: machine-wide entries without administrator
/// rights stay unticked, and protected keys are never taken over. Returns
/// a one-line summary and the backup's id.
pub fn clean_ticked(
    system: &dyn System,
    dir: &Path,
    request: &Request,
    label: &str,
    point: &str,
) -> Result<(String, Option<String>)> {
    let settings = Settings::load(dir)?;
    let report = scan(system, request, &settings.options(), &Progress::default());
    let targets: Vec<Target> = report
        .findings
        .iter()
        .filter(|finding| finding.ticked)
        .map(|finding| Target {
            key: finding.key.clone(),
            value: finding.value.clone(),
            category: Some(finding.category),
            detail: finding.detail.clone(),
        })
        .collect();
    if targets.is_empty() {
        return Ok(("Nothing to remove: the registry is clean.".into(), None));
    }
    let mut notes = Vec::new();
    if settings.full_backup
        && system.is_elevated()
        && let Err(error) = crate::backup::back_up_full(
            system,
            &settings.backups_dir(dir),
            "Full backup before cleaning",
        )
    {
        notes.push(format!("no full backup ({error})"));
    }
    if settings.restore_point
        && system.is_elevated()
        && let Err(error) = system.create_restore_point(point)
    {
        notes.push(format!("no restore point ({error})"));
    }
    let total = targets.len();
    let removal = remove(
        system,
        &crate::settings::backups_dir(dir),
        BackupKind::Clean,
        label,
        targets,
        false,
    )?;
    let mut summary = format!("Removed {} of {total} entries.", removal.removed);
    if !removal.failed.is_empty() {
        notes.push(format!("{} could not be removed", removal.failed.len()));
    }
    if !notes.is_empty() {
        summary.push_str(&format!(" Note: {}.", notes.join("; ")));
    }
    Ok((summary, removal.backup))
}

fn backup(
    system: &dyn System,
    dir: &Path,
    name: &str,
    key: &str,
) -> Result<(String, Option<String>)> {
    let key = parse_key(key).map_err(Error::Invalid)?;
    let summary = back_up_key(
        system,
        &crate::settings::backups_dir(dir),
        &key,
        &format!("Auto backup: {name}"),
    );
    let _ = system.release_account_hives();
    let summary = summary?;
    Ok((format!("Backed up {}.", key.display()), Some(summary.id)))
}
