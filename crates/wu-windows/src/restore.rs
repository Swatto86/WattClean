//! System Restore points and delete-at-restart.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};
use windows_sys::Win32::System::Restore::{
    APPLICATION_UNINSTALL, BEGIN_SYSTEM_CHANGE, END_SYSTEM_CHANGE, RESTOREPOINTINFOW,
    SRSetRestorePointW, STATEMGRSTATUS,
};

use crate::privilege::wide;

/// Ask System Restore for a restore point, as an uninstaller would. Fails
/// with the reason Windows gives, such as System Restore being turned off.
pub fn create_restore_point(description: &str) -> io::Result<()> {
    // The structure is packed, so its text field is filled before it is built.
    let mut text = [0u16; 256];
    for (slot, unit) in text.iter_mut().zip(description.encode_utf16().take(255)) {
        *slot = unit;
    }
    let begin = RESTOREPOINTINFOW {
        dwEventType: BEGIN_SYSTEM_CHANGE,
        dwRestorePtType: APPLICATION_UNINSTALL,
        llSequenceNumber: 0,
        szDescription: text,
    };
    let mut status = STATEMGRSTATUS {
        nStatus: 0,
        llSequenceNumber: 0,
    };
    // SAFETY: both pointers refer to initialised structures owned by this frame.
    if unsafe { SRSetRestorePointW(&begin, &mut status) } == 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status.nStatus).unwrap_or(-1),
        ));
    }
    let end = RESTOREPOINTINFOW {
        dwEventType: END_SYSTEM_CHANGE,
        llSequenceNumber: status.llSequenceNumber,
        ..begin
    };
    // SAFETY: as above; this closes the change the first call opened.
    if unsafe { SRSetRestorePointW(&end, &mut status) } == 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status.nStatus).unwrap_or(-1),
        ));
    }
    Ok(())
}

pub fn delete_on_reboot(path: &Path) -> io::Result<()> {
    let source = wide(OsStr::new(path.as_os_str()));
    // SAFETY: `source` is NUL-terminated and outlives the call; a null
    // destination with this flag means "delete at restart".
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            std::ptr::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
