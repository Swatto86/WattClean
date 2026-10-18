//! Manual check of schedules on this PC, without administrator rights.
//!
//! 1. Puts one task in Task Scheduler (`\WattClean\Job <pid>`, starting
//!    in 2030 so it never runs), reads it back through `schtasks`, checks it
//!    starts the installed app with `--run-job`, and deletes it again.
//! 2. Runs the installed app headless, as the task would, on a temporary
//!    data folder holding one Auto Backup of `HKCU\Control Panel\Desktop`
//!    (a read-only job), and checks the job recorded its run and a backup.
//!    The run ends with a Windows notification, "Schedule check" ran.
//!
//! Touches nothing else. Each step goes to `schedule_check-report.txt`
//! beside the executable as it happens.
//!
//! `schedule_check.exe ["C:\Program Files\WattClean\wattclean.exe"]`

#[cfg(windows)]
mod check {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use wrc_core::System;
    use wrc_core::jobs::{self, Frequency, Job, JobInput, Schedule, Work, task_name, task_xml};
    use wrc_windows::WindowsSystem;

    pub struct Log(PathBuf);

    impl Log {
        pub fn new() -> Self {
            let path = std::env::current_exe()
                .ok()
                .and_then(|exe| {
                    exe.parent()
                        .map(|dir| dir.join("schedule_check-report.txt"))
                })
                .unwrap_or_else(|| PathBuf::from("schedule_check-report.txt"));
            let _ = std::fs::write(&path, "");
            Self(path)
        }

        pub fn line(&self, text: &str) {
            println!("{text}");
            if let Ok(mut file) = OpenOptions::new().append(true).open(&self.0) {
                let _ = writeln!(file, "{text}");
            }
        }
    }

    fn input() -> JobInput {
        JobInput {
            id: None,
            name: "Schedule check".into(),
            work: Work::Backup {
                key: r"HKEY_CURRENT_USER\Control Panel\Desktop".into(),
            },
            schedule: Schedule {
                frequency: Frequency::Week { weekday: 2 },
                time: "04:15".into(),
            },
            enabled: true,
        }
    }

    /// Put a never-due task in Task Scheduler, read it back, delete it.
    fn task(system: &WindowsSystem, exe: &Path, log: &Log) -> Result<(), String> {
        let job = Job {
            id: std::process::id().to_string(),
            name: "Schedule check".into(),
            work: input().work,
            schedule: input().schedule,
            enabled: true,
            scheduled: false,
            problem: None,
            last: None,
        };
        let name = task_name(&job.id);
        system
            .schedule_task(&name, &task_xml(&job, exe, false, "2030-01-01"))
            .map_err(|error| format!("Task Scheduler refused the task: {error}"))?;
        let query = Command::new("schtasks.exe")
            .args(["/Query", "/TN", &name, "/XML"])
            .output()
            .map_err(|error| format!("schtasks: {error}"))?;
        let xml = String::from_utf8_lossy(&query.stdout).into_owned();
        let deleted = system.delete_task(&name);
        let gone = !Command::new("schtasks.exe")
            .args(["/Query", "/TN", &name])
            .output()
            .is_ok_and(|output| output.status.success());
        log.line(&format!(
            "task created, read back {} bytes, deleted: {deleted:?}, gone: {gone}",
            xml.len()
        ));
        let wanted = [
            format!("<Command>{}</Command>", exe.display()),
            format!("<Arguments>--run-job {}</Arguments>", job.id),
            "<Wednesday />".to_owned(),
            "<StartWhenAvailable>true</StartWhenAvailable>".to_owned(),
        ];
        // Least privilege is Task Scheduler's default, which it then omits.
        if xml.contains("<RunLevel>HighestAvailable</RunLevel>") {
            return Err("an unelevated schedule asked for the highest rights".into());
        }
        for text in &wanted {
            if !xml.contains(text.as_str()) {
                return Err(format!("the task Windows holds lacks {text}:\n{xml}"));
            }
        }
        if !gone {
            return Err("the task is still there after deleting it".into());
        }
        Ok(())
    }

    /// Run the installed app headless on a temporary data folder.
    fn headless(system: &WindowsSystem, exe: &Path, log: &Log) -> Result<(), String> {
        // The app keeps the registry cleaner's files in `registry` inside
        // its data folder.
        let root = std::env::temp_dir().join(format!("wrc-schedule-check-{}", std::process::id()));
        let data = root.join("registry");
        std::fs::create_dir_all(&data).map_err(|error| error.to_string())?;
        let job = jobs::save_job(
            system,
            &data,
            input(),
            Err("not scheduled by this check".into()),
        )
        .map_err(|error| error.to_string())?;
        let output = Command::new(exe)
            .args(["--run-job", &job.id])
            .env("WATTCLEAN_DATA_DIR", &root)
            .output()
            .map_err(|error| format!("could not start {}: {error}", exe.display()))?;
        let saved = jobs::list(&data).map_err(|error| error.to_string())?;
        let last = saved.first().and_then(|job| job.last.clone());
        let backups =
            wrc_core::backup::list(&data.join("backups")).map_err(|error| error.to_string())?;
        log.line(&format!(
            "headless run: exit {:?}, last run {last:?}, {} backups",
            output.status.code(),
            backups.len()
        ));
        let _ = std::fs::remove_dir_all(&root);
        match last {
            Some(last) if last.ok && output.status.success() && backups.len() == 1 => Ok(()),
            _ => Err("the headless run did not record a good run and one backup".into()),
        }
    }

    pub fn run(log: &Log) -> Result<(), String> {
        let exe = std::env::args().nth(1).map_or_else(
            || PathBuf::from(r"C:\Program Files\WattClean\wattclean.exe"),
            PathBuf::from,
        );
        if !exe.is_file() {
            return Err(format!("{} is not installed", exe.display()));
        }
        let system = WindowsSystem::new();
        task(&system, &exe, log)?;
        log.line("task: OK");
        headless(&system, &exe, log)?;
        log.line("headless: OK");
        log.line("SCHEDULE CHECK PASSED");
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let log = check::Log::new();
    match check::run(&log) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            log.line(&format!("SCHEDULE CHECK FAILED: {error}"));
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() {}
