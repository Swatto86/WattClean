//! Manual check, as administrator, that the Registry Watcher names the
//! process behind each change through event tracing on this PC.
//!
//! Watches a new key of its own, `HKCU\Software\WattClean-TraceCheck-<pid>`,
//! with the real watcher (logs in a temporary folder); has `reg.exe` set a
//! value, create a subkey and delete it; sets a value itself, through a
//! handle opened before tracing started (named from Windows' handle table);
//! and checks each logged change names the process that made it. Then measures the
//! processor time tracing costs for ten quiet seconds, and deletes the key
//! whatever happened. Touches nothing else. Each step goes to
//! `trace_check-report.txt` beside the executable as it happens.
//!
//! Run as administrator: `trace_check.exe`

#[cfg(windows)]
mod check {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    use wrc_core::model::{RawValue, RegPath};
    use wrc_core::trace::TraceState;
    use wrc_core::watch::{Event, Op, Watcher};
    use wrc_core::{Hive, System};
    use wrc_windows::WindowsSystem;

    pub struct Log(PathBuf);

    impl Log {
        pub fn new() -> Self {
            let path = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("trace_check-report.txt")))
                .unwrap_or_else(|| PathBuf::from("trace_check-report.txt"));
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

    /// This process's processor time so far, in milliseconds.
    fn cpu_ms() -> u64 {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut ended, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: the pseudo-handle needs no closing; all four out-pointers are valid.
        unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut ended,
                &mut kernel,
                &mut user,
            )
        };
        let ticks =
            |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        (ticks(kernel) + ticks(user)) / 10_000
    }

    /// `reg.exe` with these arguments; its process id.
    fn reg(args: &[&str]) -> Result<u32, String> {
        let child = Command::new("reg.exe")
            .args(args)
            .stdout(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("reg.exe: {error}"))?;
        let pid = child.id();
        let output = child
            .wait_with_output()
            .map_err(|error| format!("reg.exe: {error}"))?;
        if !output.status.success() {
            return Err(format!("reg.exe {args:?} failed"));
        }
        Ok(pid)
    }

    /// Wait for a logged change matching `wanted`.
    fn logged(
        watcher: &Watcher,
        key: &RegPath,
        wanted: impl Fn(&Event) -> bool,
    ) -> Result<Event, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let events = watcher.events(key, 0).map_err(|error| error.to_string())?;
            if let Some(event) = events.into_iter().find(|event| wanted(event)) {
                return Ok(event);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // What was logged instead, so a failure says why.
        let logged: Vec<String> = watcher
            .events(key, 0)
            .unwrap_or_default()
            .iter()
            .map(|event| {
                format!(
                    "{:?} {} {:?} {:?} -> {:?} by {:?}",
                    event.change.op,
                    event.change.key.display(),
                    event.change.value,
                    event.change.old,
                    event.change.new,
                    event.actor
                )
            })
            .collect();
        Err(format!(
            "the change was not logged within 10 seconds; the log holds:\n  {}",
            logged.join("\n  ")
        ))
    }

    fn expect(log: &Log, what: &str, event: &Event, pid: u32, process: &str) -> Result<(), String> {
        log.line(&format!("{what}: {:?}", event.actor));
        match &event.actor {
            Some(actor) if actor.pid == pid && actor.process.eq_ignore_ascii_case(process) => {
                Ok(())
            }
            _ => Err(format!("{what} was not named as {process} (pid {pid})")),
        }
    }

    /// A value flipped and straight back: each write is logged with its
    /// program, whether or not a reading saw the middle, and none twice.
    fn flips(log: &Log, watcher: &Watcher, key: &RegPath, typed: &str) -> Result<(), String> {
        let first = reg(&[
            "add",
            typed,
            "/v",
            "Flip",
            "/t",
            "REG_DWORD",
            "/d",
            "2",
            "/f",
        ])?;
        let second = reg(&[
            "add",
            typed,
            "/v",
            "Flip",
            "/t",
            "REG_DWORD",
            "/d",
            "1",
            "/f",
        ])?;
        let by = |pid: u32| {
            move |event: &Event| {
                event.change.value.as_deref() == Some("Flip")
                    && event.actor.as_ref().is_some_and(|actor| actor.pid == pid)
            }
        };
        let flipped = logged(watcher, key, by(first))?;
        expect(log, "flipped by reg.exe", &flipped, first, "reg.exe")?;
        let back = logged(watcher, key, by(second))?;
        expect(log, "flipped back by reg.exe", &back, second, "reg.exe")?;
        // Let a late reading finish, then look for doubles.
        std::thread::sleep(Duration::from_secs(2));
        let flips: Vec<Event> = watcher
            .events(key, 0)
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|event| event.change.value.as_deref() == Some("Flip"))
            .collect();
        for event in &flips {
            log.line(&format!(
                "  Flip: {:?} {:?} -> {:?} by {:?}",
                event.change.op, event.change.old, event.change.new, event.actor
            ));
        }
        if back.seq < flipped.seq {
            return Err("the flips were logged out of order".into());
        }
        if flips.iter().any(|event| event.actor.is_none()) || flips.len() != 2 {
            return Err(format!(
                "expected two Flip entries, both named; got {}",
                flips.len()
            ));
        }

        Ok(())
    }

    fn changes(log: &Log, system: &Arc<dyn System>, key: &RegPath) -> Result<(), String> {
        let data = tempfile::tempdir().map_err(|error| error.to_string())?;
        let watcher =
            Watcher::start(Arc::clone(system), data.path()).map_err(|error| error.to_string())?;
        watcher
            .add(key.clone())
            .map_err(|error| error.to_string())?;
        let state = watcher.tracing();
        log.line(&format!("tracing: {state:?}"));
        let (shown, handles) = wrc_windows::handle_addresses();
        log.line(&format!(
            "handle table: {shown} of this process's {handles} handles show their object address"
        ));
        if state != TraceState::On {
            return Err("tracing did not start".into());
        }
        // Let the watch take its first reading.
        std::thread::sleep(Duration::from_secs(1));
        let typed = format!(r"HKCU\{}", key.path);
        let sub = format!(r"{typed}\Sub");

        let pid = reg(&[
            "add", &typed, "/v", "Probe", "/t", "REG_SZ", "/d", "1", "/f",
        ])?;
        let event = logged(&watcher, key, |event| {
            event.change.value.as_deref() == Some("Probe")
        })?;
        expect(log, "value set by reg.exe", &event, pid, "reg.exe")?;

        let pid = reg(&["add", &sub, "/f"])?;
        let event = logged(&watcher, key, |event| {
            event.change.op == Op::KeyCreated && event.change.key.name() == "Sub"
        })?;
        expect(log, "key created by reg.exe", &event, pid, "reg.exe")?;

        system
            .write_value(key, &RawValue::string("Own", "1"))
            .map_err(|error| error.to_string())?;
        let event = logged(&watcher, key, |event| {
            event.change.value.as_deref() == Some("Own")
        })?;
        expect(
            log,
            "value set by this check",
            &event,
            std::process::id(),
            "trace_check.exe",
        )?;

        let pid = reg(&["delete", &sub, "/f"])?;
        let event = logged(&watcher, key, |event| event.change.op == Op::KeyDeleted)?;
        expect(log, "key deleted by reg.exe", &event, pid, "reg.exe")?;

        flips(log, &watcher, key, &typed)?;

        let before = cpu_ms();
        std::thread::sleep(Duration::from_secs(10));
        let used = cpu_ms() - before;
        log.line(&format!(
            "processor time while tracing, 10 quiet seconds: {used} ms ({:.1}% of one core)",
            used as f64 / 100.0
        ));
        watcher.stop_all();
        let before = cpu_ms();
        std::thread::sleep(Duration::from_secs(5));
        log.line(&format!(
            "after tracing stopped, 5 seconds: {} ms",
            cpu_ms() - before
        ));
        Ok(())
    }

    pub fn run(log: &Log) -> Result<(), String> {
        if !wrc_windows::is_elevated() {
            return Err("run this as administrator".into());
        }
        let system: Arc<dyn System> = Arc::new(WindowsSystem::new());
        let key = RegPath::new(
            Hive::CurrentUser,
            &format!(r"Software\WattClean-TraceCheck-{}", std::process::id()),
        );
        system
            .create_key(&key)
            .map_err(|error| format!("could not create {}: {error}", key.display()))?;
        let outcome = changes(log, &system, &key);
        log.line(&format!(
            "clean-up: delete {}: {:?}",
            key.display(),
            system.delete_key_tree(&key)
        ));
        outcome
    }
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let log = check::Log::new();
    match check::run(&log) {
        Ok(()) => {
            log.line("TRACE CHECK PASSED");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            log.line(&format!("TRACE CHECK FAILED: {error}"));
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() {}
