//! System Restore points.

use std::io;

use windows_sys::Win32::System::Restore::{
    BEGIN_SYSTEM_CHANGE, END_SYSTEM_CHANGE, MODIFY_SETTINGS, RESTOREPOINTINFOW, SRSetRestorePointW,
    STATEMGRSTATUS,
};

/// Ask System Restore for a restore point marked as a settings change. Fails
/// with the reason Windows gives, such as System Restore being turned off.
pub fn create_restore_point(description: &str) -> io::Result<()> {
    // The structure is packed, so its text field is filled before it is built.
    let mut text = [0u16; 256];
    for (slot, unit) in text.iter_mut().zip(description.encode_utf16().take(255)) {
        *slot = unit;
    }
    let begin = RESTOREPOINTINFOW {
        dwEventType: BEGIN_SYSTEM_CHANGE,
        dwRestorePtType: MODIFY_SETTINGS,
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
