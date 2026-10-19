//! Scheduled jobs: Auto Clean (scan and remove what a scan ticks, with a
//! backup) and Auto Backup (back up a key). Each job is a Task Scheduler
//! task that starts the installed app headless with `--run-job <id>`, as
//! the signed-in person, so it reaches their own registry. The list lives
//! in `jobs.json`; the task is created, replaced or deleted with it.

mod run;
mod task;
#[cfg(test)]
#[cfg(windows)]
mod tests;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub use run::{clean_ticked, notice, run, run_scheduled};
pub(crate) use task::escape;
pub use task::{task_name, task_xml};

use crate::keyname::parse_key;
use crate::scan::{Category, Mode};
use crate::store::{read_json, write_json};
use crate::system::System;
use crate::{Error, Result};

pub const FILE: &str = "jobs.json";
const MAX_JOBS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum Work {
    /// Scan in this mode and remove what the scan ticks.
    Clean {
        mode: Mode,
        #[serde(default)]
        categories: Vec<Category>,
        #[serde(default)]
        all_accounts: bool,
    },
    /// Back up this key with everything beneath it.
    Backup { key: String },
    /// Save the whole registry's hives (administrator rights).
    FullBackup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// No `deny_unknown_fields` here or on `Schedule`: serde cannot combine it
// with the flattened frequency. `JobInput` still refuses unknown fields.
#[serde(rename_all = "camelCase", tag = "every")]
pub enum Frequency {
    Day,
    /// 0 is Monday, 6 Sunday.
    Week {
        weekday: u8,
    },
    /// Day 1 to 28, so every month has it.
    Month {
        day: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    #[serde(flatten)]
    pub frequency: Frequency,
    /// `HH:MM`, 24-hour, local time.
    pub time: String,
}

/// What the last run did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    /// Seconds since the Unix epoch.
    pub time: u64,
    pub ok: bool,
    pub summary: String,
    pub backup: Option<String>,
}

/// A job as the page edits it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobInput {
    /// `None` for a new job.
    pub id: Option<String>,
    pub name: String,
    pub work: Work,
    pub schedule: Schedule,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub name: String,
    pub work: Work,
    pub schedule: Schedule,
    pub enabled: bool,
    /// Whether Task Scheduler holds a task for it now.
    pub scheduled: bool,
    /// Why it is not scheduled, when it should be.
    pub problem: Option<String>,
    pub last: Option<LastRun>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct JobFile {
    version: u32,
    jobs: Vec<Job>,
}

pub fn list(dir: &Path) -> Result<Vec<Job>> {
    Ok(load(dir)?.jobs)
}

fn load(dir: &Path) -> Result<JobFile> {
    Ok(read_json(&dir.join(FILE))?.unwrap_or(JobFile {
        version: 1,
        jobs: Vec::new(),
    }))
}

fn save(dir: &Path, file: &JobFile) -> Result<()> {
    write_json(&dir.join(FILE), file)
}

/// Where Windows should start the app from, or why it cannot.
pub type Launcher<'a> = std::result::Result<&'a Path, String>;

/// Create or change a job, then put its task in step with it. A job that
/// cannot be scheduled (not the installed app) is kept, with the reason.
pub fn save_job(
    system: &dyn System,
    dir: &Path,
    input: JobInput,
    launcher: Launcher,
) -> Result<Job> {
    validate(&input)?;
    if input.work == Work::FullBackup && !system.is_elevated() {
        return Err(Error::Invalid(
            "A full backup needs administrator rights: restart as administrator to schedule one."
                .into(),
        ));
    }
    let mut file = load(dir)?;
    let id = match &input.id {
        Some(id) => {
            if !file.jobs.iter().any(|job| job.id == *id) {
                return Err(Error::Invalid("That schedule no longer exists.".into()));
            }
            id.clone()
        }
        None => {
            if file.jobs.len() >= MAX_JOBS {
                return Err(Error::Invalid(format!(
                    "At most {MAX_JOBS} schedules can be kept."
                )));
            }
            new_id(&file)
        }
    };
    let last = file
        .jobs
        .iter()
        .find(|job| job.id == id)
        .and_then(|job| job.last.clone());
    let mut job = Job {
        id,
        name: input.name.trim().to_owned(),
        work: input.work,
        schedule: input.schedule,
        enabled: input.enabled,
        scheduled: false,
        problem: None,
        last,
    };
    sync_task(system, &mut job, launcher);
    file.jobs.retain(|old| old.id != job.id);
    file.jobs.push(job.clone());
    file.jobs.sort_by_key(|job| job.name.to_lowercase());
    save(dir, &file)?;
    Ok(job)
}

/// Pause or resume a job.
pub fn set_enabled(
    system: &dyn System,
    dir: &Path,
    id: &str,
    enabled: bool,
    launcher: Launcher,
) -> Result<Job> {
    let mut file = load(dir)?;
    let job = file
        .jobs
        .iter_mut()
        .find(|job| job.id == id)
        .ok_or_else(|| Error::Invalid("That schedule no longer exists.".into()))?;
    job.enabled = enabled;
    sync_task(system, job, launcher);
    let job = job.clone();
    save(dir, &file)?;
    Ok(job)
}

/// Delete a job and its task.
pub fn delete(system: &dyn System, dir: &Path, id: &str) -> Result<()> {
    let mut file = load(dir)?;
    let before = file.jobs.len();
    file.jobs.retain(|job| job.id != id);
    if file.jobs.len() == before {
        return Err(Error::Invalid("That schedule no longer exists.".into()));
    }
    save(dir, &file)?;
    system.delete_task(&task_name(id)).map_err(|source| {
        Error::Invalid(format!("Its Windows task could not be removed: {source}"))
    })
}

/// Create, replace or delete the job's task to match it.
fn sync_task(system: &dyn System, job: &mut Job, launcher: Launcher) {
    job.problem = None;
    if !job.enabled {
        job.scheduled = false;
        if let Err(error) = system.delete_task(&task_name(&job.id)) {
            job.problem = Some(format!("Its Windows task could not be removed: {error}"));
        }
        return;
    }
    let result = launcher
        .map_err(|reason| format!("Not scheduled: {reason}"))
        .and_then(|exe| {
            let xml = task_xml(job, exe, system.is_elevated(), &today());
            system
                .schedule_task(&task_name(&job.id), &xml)
                .map_err(|error| format!("Windows refused the task: {error}"))
        });
    job.scheduled = result.is_ok();
    job.problem = result.err();
}

fn validate(input: &JobInput) -> Result<()> {
    let invalid = |text: &str| Err(Error::Invalid(text.to_owned()));
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return invalid("Give the schedule a name of up to 80 characters.");
    }
    if let Some(id) = &input.id
        && (id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()))
    {
        return invalid("That schedule is not valid.");
    }
    match &input.work {
        Work::Clean {
            mode: Mode::Custom,
            categories,
            ..
        } if categories.is_empty() => {
            return invalid("Tick at least one category for a custom clean.");
        }
        Work::Backup { key } => {
            let parsed = parse_key(key).map_err(Error::Invalid)?;
            if parsed.path.is_empty() {
                return invalid("Back up a key inside the hive, not a whole hive.");
            }
        }
        Work::Clean { .. } | Work::FullBackup => {}
    }
    match input.schedule.frequency {
        Frequency::Week { weekday } if weekday > 6 => return invalid("Choose a day of the week."),
        Frequency::Month { day } if !(1..=28).contains(&day) => {
            return invalid("Choose a day of the month from 1 to 28.");
        }
        _ => {}
    }
    if parse_time(&input.schedule.time).is_none() {
        return invalid("Give the time as hours and minutes, such as 03:00.");
    }
    Ok(())
}

/// `HH:MM` as hours and minutes.
pub(crate) fn parse_time(text: &str) -> Option<(u8, u8)> {
    let (hours, minutes) = text.split_once(':')?;
    if hours.len() != 2 || minutes.len() != 2 {
        return None;
    }
    let hours: u8 = hours.parse().ok()?;
    let minutes: u8 = minutes.parse().ok()?;
    (hours < 24 && minutes < 60).then_some((hours, minutes))
}

fn new_id(file: &JobFile) -> String {
    let mut id = now().max(1);
    while file.jobs.iter().any(|job| job.id == id.to_string()) {
        id += 1;
    }
    id.to_string()
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Today's date (UTC) as `YYYY-MM-DD`, the day schedules start from.
fn today() -> String {
    let days = i64::try_from(now() / 86_400).unwrap_or(0);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Record a run's outcome on its job.
fn record(dir: &Path, id: &str, last: LastRun) -> Result<()> {
    let mut file = load(dir)?;
    if let Some(job) = file.jobs.iter_mut().find(|job| job.id == id) {
        job.last = Some(last);
        save(dir, &file)?;
    }
    Ok(())
}
