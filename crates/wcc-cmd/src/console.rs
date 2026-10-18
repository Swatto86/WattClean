//! `wattclean-cmd disk`: read the arguments, open this PC, run, and return
//! the exit code.

/// Runs the disk cleaner's console commands; returns the exit code.
pub fn console(args: &[String]) -> i32 {
    let command = match crate::parse(args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}\n\n{}", crate::USAGE);
            return 2;
        }
    };
    let data_dir = match wcc_core::store::data_dir() {
        Ok(dir) => dir,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    run(&command, &data_dir)
}

#[cfg(windows)]
fn run(command: &crate::Command, data_dir: &std::path::Path) -> i32 {
    let system = wcc_windows::WindowsSystem::new();
    let mut out = std::io::stdout();
    crate::execute(command, &system, data_dir, &mut out).unwrap_or_else(|error| {
        eprintln!("{error}");
        1
    })
}

#[cfg(not(windows))]
fn run(_command: &crate::Command, _data_dir: &std::path::Path) -> i32 {
    eprintln!("WattClean runs on Windows.");
    2
}
