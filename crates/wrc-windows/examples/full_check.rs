//! Manual check, as administrator, of the full registry backup.
//!
//! Makes a real full backup (SOFTWARE, SYSTEM, DEFAULT and this account's
//! two hives) into a temporary folder, with the backup and restore
//! privileges both on (as after taking ownership, which once broke saving),
//! reports the sizes, and deletes it again. Nothing in the registry
//! changes.
//!
//! Each step goes to `full_check-report.txt` beside the executable.
//!
//! Run as administrator: `full_check.exe`

#[cfg(windows)]
mod check {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use wrc_core::backup::back_up_full;
    use wrc_windows::WindowsSystem;

    pub struct Log(PathBuf);

    impl Log {
        pub fn new() -> Self {
            let path = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("full_check-report.txt")))
                .unwrap_or_else(|| PathBuf::from("full_check-report.txt"));
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

    fn real_backup(system: &WindowsSystem, folder: &Path, log: &Log) -> Result<(), String> {
        let started = Instant::now();
        let summary = back_up_full(system, folder, "Full check").map_err(|e| e.to_string())?;
        let place = folder.join(&summary.id);
        let mut total = 0;
        for entry in std::fs::read_dir(&place)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            total += size;
            log.line(&format!(
                "  {}: {:.1} MB",
                entry.file_name().to_string_lossy(),
                size as f64 / 1_048_576.0
            ));
        }
        log.line(&format!(
            "full backup: {} hives, {:.1} MB, {:.1} s",
            summary.items,
            total as f64 / 1_048_576.0,
            started.elapsed().as_secs_f64()
        ));
        if summary.items != 5 {
            return Err("not every hive was saved".into());
        }
        Ok(())
    }

    pub fn run(log: &Log) -> Result<(), String> {
        if !wrc_windows::is_elevated() {
            return Err("run this as administrator".into());
        }
        let system = WindowsSystem::new();
        let folder = std::env::temp_dir().join(format!("wrc-full-check-{}", std::process::id()));
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        for privilege in ["SeBackupPrivilege", "SeRestorePrivilege"] {
            wrc_windows::enable_privilege(privilege).map_err(|e| e.to_string())?;
        }
        let outcome = real_backup(&system, &folder.join("backups"), log);
        let removed = std::fs::remove_dir_all(&folder);
        log.line(&format!(
            "clean-up: removed {}: {removed:?}",
            folder.display()
        ));
        outcome
    }
}
#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let log = check::Log::new();
    match check::run(&log) {
        Ok(()) => {
            log.line("FULL BACKUP CHECK PASSED");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            log.line(&format!("FULL BACKUP CHECK FAILED: {error}"));
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() {}
