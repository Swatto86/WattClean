//! Ctrl+C in the console program asks the work to stop instead of ending
//! the process at once, so a free-space wipe can give the drive's space
//! back and a shred is not cut off halfway through a file.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use windows_sys::Win32::System::Console::{CTRL_CLOSE_EVENT, SetConsoleCtrlHandler};
use windows_sys::core::BOOL;

static STOP: AtomicBool = AtomicBool::new(false);

/// From now on the first Ctrl+C or Ctrl+Break sets the returned flag; a
/// second one ends the program as usual. Closing the console window sets
/// it too: Windows then ends the program after a few seconds, which the
/// handler waits out so the work can stop first.
pub fn stop_on_ctrl_c() -> io::Result<&'static AtomicBool> {
    // SAFETY: `on_event` is a plain function that lives as long as the process.
    if unsafe { SetConsoleCtrlHandler(Some(on_event), 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(&STOP)
}

extern "system" fn on_event(event: u32) -> BOOL {
    if STOP.swap(true, Ordering::SeqCst) && event != CTRL_CLOSE_EVENT {
        return 0;
    }
    if event == CTRL_CLOSE_EVENT {
        // The process ends when this returns; it may finish sooner itself.
        std::thread::sleep(Duration::from_millis(4500));
    }
    1
}
