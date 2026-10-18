//! Windows' change notification on a key: an event signalled when anything
//! beneath the key changes, re-armed after each signal.

use std::io;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Registry::{
    REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, REG_NOTIFY_THREAD_AGNOSTIC,
    RegNotifyChangeKeyValue,
};
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};
use winreg::RegKey;
use winreg::enums::{KEY_NOTIFY, KEY_READ, KEY_WOW64_64KEY};
use wrc_core::model::RegPath;
use wrc_core::system::KeyWatch;

pub struct Notify {
    key: RegKey,
    event: HANDLE,
}

// SAFETY: the key handle and the event handle are kernel handles any thread
// may use; the struct owns both and nothing else.
unsafe impl Send for Notify {}

impl Notify {
    pub fn open(root: &RegKey, key: &RegPath) -> io::Result<Self> {
        let key =
            root.open_subkey_with_flags(&key.path, KEY_NOTIFY | KEY_READ | KEY_WOW64_64KEY)?;
        // SAFETY: no security attributes or name; a manual-reset event that
        // starts unsignalled.
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let notify = Self { key, event };
        notify.arm()?;
        Ok(notify)
    }

    fn arm(&self) -> io::Result<()> {
        // SAFETY: both handles are open and owned by `self`; asynchronous
        // registration signals `event` once, on the next change beneath the key.
        let status = unsafe {
            RegNotifyChangeKeyValue(
                self.key.raw_handle() as _,
                1,
                REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC,
                self.event,
                1,
            )
        };
        if status != 0 {
            return Err(io::Error::from_raw_os_error(
                i32::try_from(status).unwrap_or(-1),
            ));
        }
        Ok(())
    }
}

impl KeyWatch for Notify {
    fn wait(&mut self, timeout: Duration) -> io::Result<bool> {
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: `event` is an open event handle owned by `self`.
        match unsafe { WaitForSingleObject(self.event, millis) } {
            WAIT_OBJECT_0 => {
                // SAFETY: as above.
                if unsafe { ResetEvent(self.event) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                // Re-arm before the caller reads the key, so a change made
                // while it reads is signalled again.
                self.arm()?;
                Ok(true)
            }
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

impl Drop for Notify {
    fn drop(&mut self) {
        // SAFETY: the event handle is owned by `self` and closed once.
        unsafe { CloseHandle(self.event) };
    }
}
