//! Manual check, as administrator, of starting with Windows elevated.
//!
//! With no copy of WattClean running and start-with-Windows off: switches
//! it on as the app does (the sign-in task), reads the task back, starts it
//! now as sign-in would, checks the installed app came up elevated and
//! hidden, then opens the Start menu shortcut with ordinary rights (through
//! File Explorer) and checks the elevated copy showed its window and no
//! second copy stayed. Then ends the copy it started and switches
//! start-with-Windows off again, whatever happened. Each step goes to
//! `autostart_check-report.txt` beside the executable.
//!
//! Run as administrator: `autostart_check.exe ["C:\Program Files\WattClean\wattclean.exe"]`

#[cfg(windows)]
mod check {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };
    use wrc_core::autostart::{self, TASK};
    use wrc_windows::WindowsSystem;

    const SHORTCUT: &str = r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\WattClean.lnk";

    pub struct Log(PathBuf);

    impl Log {
        pub fn new() -> Self {
            let path = std::env::current_exe()
                .ok()
                .and_then(|exe| {
                    exe.parent()
                        .map(|dir| dir.join("autostart_check-report.txt"))
                })
                .unwrap_or_else(|| PathBuf::from("autostart_check-report.txt"));
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

    /// Running copies of `exe`, by process id.
    fn copies(exe: &Path) -> Vec<u32> {
        let filter = format!(
            "$p = '{}'; Get-CimInstance Win32_Process -Filter \"Name = 'wattclean.exe'\" | Where-Object ExecutablePath -eq $p | ForEach-Object ProcessId",
            exe.display()
        );
        Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &filter])
            .output()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter_map(|line| line.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn elevated(pid: u32) -> Option<bool> {
        // SAFETY: plain queries; both handles are checked and closed.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut token: HANDLE = std::ptr::null_mut();
            let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
            CloseHandle(process);
            if opened == 0 {
                return None;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut size = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&raw mut elevation).cast(),
                u32::try_from(std::mem::size_of::<TOKEN_ELEVATION>()).unwrap_or(0),
                &mut size,
            );
            CloseHandle(token);
            (ok != 0).then_some(elevation.TokenIsElevated != 0)
        }
    }

    struct Search {
        pid: u32,
        found: bool,
    }

    unsafe extern "system" fn visit(window: HWND, data: LPARAM) -> i32 {
        // SAFETY: `data` is the `Search` that `window_shown` passed.
        let search = unsafe { &mut *(data as *mut Search) };
        let mut owner = 0u32;
        let mut title = [0u16; 64];
        // SAFETY: a window handle from EnumWindows and buffers of the given size.
        let (visible, length) = unsafe {
            GetWindowThreadProcessId(window, &mut owner);
            (
                IsWindowVisible(window) != 0,
                GetWindowTextW(window, title.as_mut_ptr(), 64),
            )
        };
        let length = usize::try_from(length).unwrap_or(0);
        if owner == search.pid
            && visible
            && String::from_utf16_lossy(&title[..length]) == "WattClean"
        {
            search.found = true;
            return 0;
        }
        1
    }

    /// Whether process `pid` shows its main window.
    fn window_shown(pid: u32) -> bool {
        let mut search = Search { pid, found: false };
        // SAFETY: the callback only reads windows and writes `search`.
        unsafe { EnumWindows(Some(visit), (&raw mut search) as LPARAM) };
        search.found
    }

    fn wait_for(what: impl Fn() -> bool, seconds: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            if what() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        false
    }

    fn steps(system: &WindowsSystem, exe: &Path, log: &Log) -> Result<(), String> {
        let state = autostart::enable(system, exe).map_err(|error| error.to_string())?;
        log.line(&format!("switched on: {state:?}"));
        if !state.elevated {
            return Err("it was not switched on as the sign-in task".into());
        }
        let xml = Command::new("schtasks.exe")
            .args(["/Query", "/TN", TASK, "/XML"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default();
        for wanted in [
            "<LogonTrigger>",
            "<RunLevel>HighestAvailable</RunLevel>",
            "--background",
        ] {
            if !xml.contains(wanted) {
                return Err(format!("the task Windows holds lacks {wanted}:\n{xml}"));
            }
        }
        log.line("task read back: sign-in trigger, highest rights, --background");
        let started = Command::new("schtasks.exe")
            .args(["/Run", "/TN", TASK])
            .output()
            .map_err(|error| error.to_string())?;
        if !started.status.success() {
            return Err(format!(
                "the task did not start: {}",
                String::from_utf8_lossy(&started.stderr)
            ));
        }
        if !wait_for(|| copies(exe).len() == 1, 20) {
            return Err("the sign-in start did not run the app".into());
        }
        let pid = copies(exe)[0];
        std::thread::sleep(Duration::from_secs(3));
        log.line(&format!(
            "started by the task: pid {pid}, elevated {:?}, window shown {}",
            elevated(pid),
            window_shown(pid)
        ));
        if elevated(pid) != Some(true) {
            return Err("the sign-in start is not elevated".into());
        }
        if window_shown(pid) {
            return Err("the sign-in start showed its window".into());
        }
        // File Explorer opens the shortcut with ordinary rights.
        Command::new("explorer.exe")
            .arg(SHORTCUT)
            .spawn()
            .map_err(|error| error.to_string())?;
        let shown = wait_for(|| window_shown(pid), 20);
        std::thread::sleep(Duration::from_secs(3));
        let running = copies(exe);
        log.line(&format!(
            "after the Start menu shortcut: elevated copy shows its window {shown}, copies running {running:?}"
        ));
        if !shown {
            return Err("the elevated copy did not show its window".into());
        }
        if running != [pid] {
            return Err("a second copy kept running".into());
        }
        Ok(())
    }

    pub fn run(log: &Log) -> Result<(), String> {
        if !wrc_windows::is_elevated() {
            return Err("run this as administrator".into());
        }
        let exe = std::env::args().nth(1).map_or_else(
            || PathBuf::from(r"C:\Program Files\WattClean\wattclean.exe"),
            PathBuf::from,
        );
        if !exe.is_file() {
            return Err(format!("{} is not installed", exe.display()));
        }
        let system = WindowsSystem::new();
        if !copies(&exe).is_empty() {
            return Err("quit WattClean first".into());
        }
        if autostart::state(&system).on {
            return Err("start with Windows is already on; switch it off first".into());
        }
        let outcome = steps(&system, &exe, log);
        for pid in copies(&exe) {
            let ended = Command::new("taskkill.exe")
                .args(["/PID", &pid.to_string(), "/F"])
                .output()
                .is_ok_and(|output| output.status.success());
            log.line(&format!(
                "clean-up: ended the copy this check started ({pid}): {ended}"
            ));
        }
        log.line(&format!(
            "clean-up: switched off: {:?}",
            autostart::disable(&system).map_err(|error| error.to_string())
        ));
        outcome
    }
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let log = check::Log::new();
    match check::run(&log) {
        Ok(()) => {
            log.line("AUTOSTART CHECK PASSED");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            log.line(&format!("AUTOSTART CHECK FAILED: {error}"));
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() {}
