//! Manual check of removing a protected key, on this PC, as administrator.
//!
//! Creates `HKLM\SOFTWARE\WattClean-ForceCheck` (and a subkey) owned by
//! TrustedInstaller and read-only, as Windows protects its own keys; shows
//! that a plain removal is refused; removes it by taking ownership, with a
//! backup in a temporary folder; restores it and checks the original owner
//! and permissions came back; then removes the key again, whatever happened
//! (a key left by an earlier run first). Touches nothing else. Each step is
//! written to `force_check-report.txt` beside the executable as it happens.
//!
//! Run as administrator: `force_check.exe`

#[cfg(windows)]
mod check {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::PathBuf;

    use wrc_core::backup::{BackupKind, Target, remove, restore};
    use wrc_core::model::{RawValue, RegPath};
    use wrc_core::{Hive, System};
    use wrc_windows::WindowsSystem;

    const TRUSTED_INSTALLER: &str =
        "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

    /// Lines written to the report file at once, so a failure still leaves them.
    pub struct Log(PathBuf);

    impl Log {
        pub fn new() -> Self {
            let path = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("force_check-report.txt")))
                .unwrap_or_else(|| PathBuf::from("force_check-report.txt"));
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

    fn fail(what: &str, error: &dyn std::fmt::Display) -> String {
        format!("{what}: {error}")
    }

    /// Take the key over and delete it, if it is there, logging each step so
    /// a refusal shows which one Windows refused.
    fn clean_up(system: &WindowsSystem, key: &RegPath, log: &Log) {
        if system.subkeys(key).is_err() && system.key_security(key).is_err() {
            return;
        }
        log.line(&format!(
            "clean-up: take ownership: {:?}",
            system.take_ownership(key, true)
        ));
        match system.delete_key_tree(key) {
            Ok(()) => log.line("cleaned up the check key"),
            Err(error) => log.line(&format!("COULD NOT CLEAN UP the check key: {error}")),
        }
    }

    /// Create the key and its subkey, protected; their SDDL as Windows reads it.
    fn protect(system: &WindowsSystem, key: &RegPath) -> Result<(String, String), String> {
        let sub = key.child("Sub");
        system
            .write_value(&sub, &RawValue::dword("n", 1))
            .map_err(|e| fail("create", &e))?;
        system
            .write_value(key, &RawValue::string("Name", "force check"))
            .map_err(|e| fail("create", &e))?;
        let protected =
            format!("O:{TRUSTED_INSTALLER}G:SYD:P(A;CI;KR;;;BA)(A;CI;KA;;;{TRUSTED_INSTALLER})");
        for target in [&sub, key] {
            system
                .set_key_security(target, &protected)
                .map_err(|e| fail("protect", &e))?;
        }
        Ok((
            system.key_security(key).map_err(|e| fail("read", &e))?,
            system.key_security(&sub).map_err(|e| fail("read", &e))?,
        ))
    }

    pub fn run(log: &Log) -> Result<(), String> {
        let system = WindowsSystem::new();
        if !system.is_elevated() {
            return Err("run this as administrator".into());
        }
        let key = RegPath::new(Hive::LocalMachine, r"SOFTWARE\WattClean-ForceCheck");
        clean_up(&system, &key, log);
        let result = steps(&system, &key, log);
        clean_up(&system, &key, log);
        result
    }

    fn steps(system: &WindowsSystem, key: &RegPath, log: &Log) -> Result<(), String> {
        let (original, original_sub) = protect(system, key)?;
        log.line(&format!("protected as: {original}"));
        match system.delete_key_tree(key) {
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                log.line("plain removal refused, as expected");
            }
            other => return Err(format!("plain removal was not refused: {other:?}")),
        }
        let backups = std::env::temp_dir().join(format!("wrc-force-check-{}", std::process::id()));
        let attempt = |force| {
            let target = Target {
                key: key.clone(),
                value: None,
                category: None,
                detail: "force check".into(),
            };
            remove(
                system,
                &backups,
                BackupKind::Clean,
                "check",
                vec![target],
                force,
            )
            .map_err(|e| fail("remove", &e))
        };
        let plain = attempt(false)?;
        log.line(&format!(
            "without force: {} removed, failed {:?}",
            plain.removed, plain.failed
        ));
        if plain.removed != 0 {
            return Err("removed without taking ownership".into());
        }
        let forced = attempt(true)?;
        log.line(&format!(
            "with force: {} removed, {} forced, failed {:?}, warnings {:?}",
            forced.removed, forced.forced, forced.failed, forced.warnings
        ));
        if forced.forced != 1 || system.subkeys(key).is_ok() {
            return Err("the protected key was not removed".into());
        }
        let id = forced.backup.ok_or("no backup was kept")?;
        let outcome = restore(system, &backups, &id, None).map_err(|e| fail("restore", &e))?;
        log.line(&format!(
            "restored {}, skipped {:?}",
            outcome.restored, outcome.skipped
        ));
        let back = system.key_security(key).map_err(|e| fail("read", &e))?;
        let back_sub = system
            .key_security(&key.child("Sub"))
            .map_err(|e| fail("read", &e))?;
        let values = system.values(key).map_err(|e| fail("read", &e))?;
        log.line(&format!("restored as: {back}"));
        let _ = std::fs::remove_dir_all(&backups);
        if back != original || back_sub != original_sub {
            return Err("the owner and permissions did not come back as they were".into());
        }
        if values != vec![RawValue::string("Name", "force check")] {
            return Err("the values did not come back".into());
        }
        log.line("FORCE CHECK PASSED");
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let log = check::Log::new();
    match check::run(&log) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            log.line(&format!("FORCE CHECK FAILED: {error}"));
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() {}
