//! Ctrl+C in the console program asks the work to stop instead of ending
//! the process at once, so a free-space wipe can give the drive's space
//! back and a shred is not cut off halfway through a file.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

/// From now on the first Ctrl+C (or a polite kill) sets the returned flag;
/// a second one ends the program at once.
pub fn stop_on_ctrl_c() -> io::Result<&'static AtomicBool> {
    crate::sys::on_interrupt(on_signal)?;
    Ok(&STOP)
}

extern "C" fn on_signal(_signal: libc::c_int) {
    if STOP.swap(true, Ordering::SeqCst) {
        std::process::exit(130);
    }
}
