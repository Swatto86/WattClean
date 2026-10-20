//! `wattclean-cmd registry`: the real machine and the registry cleaner's
//! data folder.

/// Runs the registry cleaner's console commands; returns the exit code.
#[cfg(windows)]
pub fn console(args: &[String]) -> i32 {
    let command = match crate::parse(args) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error}\n\n{}", crate::USAGE);
            return 2;
        }
    };
    let dir = match wrc_core::store::data_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("{}: {error}", dir.display());
        return 2;
    }
    let system = wrc_windows::WindowsSystem::new();
    let machine = crate::Machine {
        system: &system,
        dir: &dir,
        app_running: app_running(),
    };
    crate::run(&command, &machine, &mut std::io::stdout())
}

#[cfg(not(windows))]
pub fn console(_args: &[String]) -> i32 {
    eprintln!("The registry cleaner runs on Windows only.");
    2
}

/// Whether WattClean is open: its single-instance mutex exists.
#[cfg(windows)]
fn app_running() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};

    let name: Vec<u16> = "co.swatto.wattclean-sim"
        .encode_utf16()
        .chain([0])
        .collect();
    // SAFETY: the name is NUL-terminated; a handle returned is closed at once.
    unsafe {
        let mutex = OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr());
        if mutex.is_null() {
            return false;
        }
        CloseHandle(mutex);
    }
    true
}
