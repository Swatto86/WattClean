//! A second launch reaching this copy when it runs as administrator. The
//! single-instance plugin passes a second launch's arguments to the first
//! copy's hidden window as `WM_COPYDATA`; Windows drops that message when
//! the sender has fewer rights (the Start menu, while the copy started at
//! sign-in runs elevated) unless the window lets it through. Only that one
//! message is let through, and the app answers it only by showing its
//! window.

use std::ffi::OsStr;
use std::io;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, FindWindowW, GetWindowThreadProcessId, MSGFLT_ALLOW, WM_COPYDATA,
};

use crate::privilege::wide;

/// Let `WM_COPYDATA` from programs with fewer rights reach this process's
/// window of this class and title.
pub fn accept_launch_messages(class: &str, title: &str) -> io::Result<()> {
    let class = wide(OsStr::new(class));
    let title = wide(OsStr::new(title));
    // SAFETY: both names are NUL-terminated and outlive the call.
    let window = unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) };
    if window.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the single-instance window was not found",
        ));
    }
    let mut owner = 0u32;
    // SAFETY: `window` is a window handle; the out-pointer is valid.
    unsafe { GetWindowThreadProcessId(window, &mut owner) };
    if owner != std::process::id() {
        return Err(io::Error::other(
            "the single-instance window belongs to another program",
        ));
    }
    // SAFETY: this process's own window; no extended result is asked for.
    let ok = unsafe {
        ChangeWindowMessageFilterEx(window, WM_COPYDATA, MSGFLT_ALLOW, std::ptr::null_mut())
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
