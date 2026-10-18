//! Schedules: saved, put in Task Scheduler only for the installed app,
//! paused and deleted with their task, and run headless.

use std::path::Path;

use super::*;
use crate::fake::FakeSystem;
use crate::model::{Hive, RawValue, RegPath};

const INSTALLED: &str = r"C:\Program Files\WattClean\wattclean.exe";

fn machine() -> (tempfile::TempDir, FakeSystem) {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(&dir.path().join("machine")).unwrap();
    (dir, system)
}

fn clean_input() -> JobInput {
    JobInput {
        id: None,
        name: "Nightly clean".into(),
        work: Work::Clean {
            mode: Mode::Safe,
            categories: Vec::new(),
            all_accounts: false,
        },
        schedule: Schedule {
            frequency: Frequency::Week { weekday: 0 },
            time: "03:30".into(),
        },
        enabled: true,
    }
}

#[test]
fn a_saved_job_gets_a_task_that_starts_the_installed_app() {
    let (dir, system) = machine();
    let job = save_job(&system, dir.path(), clean_input(), Ok(Path::new(INSTALLED))).unwrap();
    assert!(job.scheduled, "{:?}", job.problem);
    let tasks = system.tasks();
    let xml = tasks.get(&task_name(&job.id)).unwrap();
    assert!(
        xml.contains(&format!("<Arguments>--run-job {}</Arguments>", job.id)),
        "{xml}"
    );
    assert!(xml.contains(r"<Command>C:\Program Files\WattClean\wattclean.exe</Command>"));
    assert!(xml.contains("<Monday />"));
    assert!(xml.contains("T03:30:00</StartBoundary>"));
    assert!(
        xml.contains("<RunLevel>HighestAvailable</RunLevel>"),
        "elevated fake"
    );
    assert_eq!(list(dir.path()).unwrap(), vec![job]);
}

#[test]
fn a_development_build_keeps_the_job_but_schedules_nothing() {
    let (dir, system) = machine();
    let job = save_job(
        &system,
        dir.path(),
        clean_input(),
        Err("this is a development build".into()),
    )
    .unwrap();
    assert!(!job.scheduled);
    assert_eq!(
        job.problem.as_deref(),
        Some("Not scheduled: this is a development build")
    );
    assert!(system.tasks().is_empty());
}

#[test]
fn pausing_and_deleting_remove_the_task() {
    let (dir, system) = machine();
    let job = save_job(&system, dir.path(), clean_input(), Ok(Path::new(INSTALLED))).unwrap();
    let paused = set_enabled(
        &system,
        dir.path(),
        &job.id,
        false,
        Ok(Path::new(INSTALLED)),
    )
    .unwrap();
    assert!(!paused.scheduled && !paused.enabled);
    assert!(system.tasks().is_empty());
    set_enabled(&system, dir.path(), &job.id, true, Ok(Path::new(INSTALLED))).unwrap();
    assert_eq!(system.tasks().len(), 1);
    delete(&system, dir.path(), &job.id).unwrap();
    assert!(system.tasks().is_empty());
    assert!(list(dir.path()).unwrap().is_empty());
}

#[test]
fn bad_jobs_are_refused() {
    let (dir, system) = machine();
    let mut bad = clean_input();
    bad.schedule.time = "25:00".into();
    assert!(save_job(&system, dir.path(), bad, Ok(Path::new(INSTALLED))).is_err());
    let mut bad = clean_input();
    bad.schedule.frequency = Frequency::Month { day: 31 };
    assert!(save_job(&system, dir.path(), bad, Ok(Path::new(INSTALLED))).is_err());
    let mut bad = clean_input();
    bad.work = Work::Backup {
        key: r"HKCR\.txt".into(),
    };
    assert!(save_job(&system, dir.path(), bad, Ok(Path::new(INSTALLED))).is_err());
    let mut bad = clean_input();
    bad.name = " ".into();
    assert!(save_job(&system, dir.path(), bad, Ok(Path::new(INSTALLED))).is_err());
    let mut bad = clean_input();
    bad.id = Some(r"..\x".into());
    assert!(save_job(&system, dir.path(), bad, Ok(Path::new(INSTALLED))).is_err());
    assert!(serde_json::from_str::<JobInput>(r#"{"name":"x","work":{"kind":"wipe"}}"#).is_err());
}

#[test]
fn an_auto_clean_removes_what_a_scan_ticks_with_a_backup() {
    let (dir, system) = machine();
    let job = save_job(&system, dir.path(), clean_input(), Err("dev".into())).unwrap();
    let last = run(&system, dir.path(), &job.id).unwrap();
    assert!(last.ok, "{}", last.summary);
    assert_eq!(last.summary, "Removed 8 of 8 entries.");
    assert!(last.backup.is_some());
    let run_key = RegPath::new(
        Hive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Run",
    );
    let names: Vec<String> = system
        .values_of(&run_key)
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert!(!names.contains(&"FabrikamUpdater".to_owned()));
    assert!(names.contains(&"ContosoNotes".to_owned()));
    assert_eq!(list(dir.path()).unwrap()[0].last.as_ref(), Some(&last));
    let again = run(&system, dir.path(), &job.id).unwrap();
    assert_eq!(again.summary, "Nothing to remove: the registry is clean.");
}

#[test]
fn an_auto_backup_backs_up_its_key() {
    let (dir, system) = machine();
    let key = RegPath::new(Hive::CurrentUser, r"Software\Keep");
    system.put(&key, vec![RawValue::string("a", "1")]);
    let mut input = clean_input();
    input.work = Work::Backup {
        key: r"HKEY_CURRENT_USER\Software\Keep".into(),
    };
    let job = save_job(&system, dir.path(), input, Err("dev".into())).unwrap();
    let last = run(&system, dir.path(), &job.id).unwrap();
    assert_eq!(last.summary, r"Backed up HKCU\Software\Keep.");
    let backups = crate::backup::list(&dir.path().join("backups")).unwrap();
    assert_eq!(backups[0].label, "Auto backup: Nightly clean");
}

#[test]
fn a_full_backup_is_scheduled_only_as_administrator() {
    let (dir, system) = machine();
    let mut input = clean_input();
    input.work = Work::FullBackup;
    system.set_elevated(false);
    let refused = save_job(&system, dir.path(), input.clone(), Err("dev".into()));
    assert!(refused.unwrap_err().to_string().contains("administrator"));
    system.set_elevated(true);
    let job = save_job(&system, dir.path(), input, Err("dev".into())).unwrap();
    let last = run(&system, dir.path(), &job.id).unwrap();
    assert!(last.ok, "{}", last.summary);
    assert_eq!(last.summary, "Saved the whole registry (5 hives).");
    let backups = crate::backup::list(&dir.path().join("backups")).unwrap();
    assert_eq!(backups[0].label, "Auto full backup: Nightly clean");
}

#[test]
fn a_task_for_a_deleted_job_removes_itself() {
    let (dir, system) = machine();
    system.schedule_task(&task_name("123"), "<Task />").unwrap();
    assert!(run(&system, dir.path(), "123").is_err());
    assert!(system.tasks().is_empty());
}

#[test]
fn a_scheduled_run_ends_with_a_notification() {
    let (dir, system) = machine();
    system.put(&RegPath::new(Hive::CurrentUser, r"Software\Keep"), vec![]);
    let mut input = clean_input();
    input.work = Work::Backup {
        key: r"HKEY_CURRENT_USER\Software\Keep".into(),
    };
    let job = save_job(&system, dir.path(), input, Err("dev".into())).unwrap();
    let (outcome, shown) = run_scheduled(&system, dir.path(), &job.id);
    assert!(outcome.unwrap().ok);
    shown.unwrap();
    let (_, missing) = run_scheduled(&system, dir.path(), "999");
    missing.unwrap();
    assert_eq!(
        system.notifications(),
        vec![
            (
                "\"Nightly clean\" ran".to_owned(),
                r"Backed up HKCU\Software\Keep.".to_owned()
            ),
            (
                "A schedule could not run".to_owned(),
                "Schedule 999 no longer exists; its task was removed.".to_owned()
            ),
        ]
    );
}

#[test]
fn a_run_with_a_problem_says_so() {
    let last = LastRun {
        time: 0,
        ok: false,
        summary: "The key could not be read.".into(),
        backup: None,
    };
    assert_eq!(
        notice(Some("Weekly"), &Ok(last)),
        (
            "\"Weekly\" ran into a problem".to_owned(),
            "The key could not be read.".to_owned()
        )
    );
}

#[test]
fn schedule_kinds_serialise_for_the_page() {
    let json = serde_json::to_string(&Schedule {
        frequency: Frequency::Month { day: 1 },
        time: "03:00".into(),
    })
    .unwrap();
    assert_eq!(json, r#"{"every":"month","day":1,"time":"03:00"}"#);
}
