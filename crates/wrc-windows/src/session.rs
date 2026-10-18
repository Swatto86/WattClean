//! Signing out and restarting Windows, as the Registry Tuner offers after
//! a tweak that needs one. Both are planned, so Windows records them as
//! such; programs still open are asked to close as at any sign-out.

use std::io;

use windows_sys::Win32::System::Shutdown::{
    EWX_LOGOFF, EWX_REBOOT, ExitWindowsEx, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER,
    SHTDN_REASON_MINOR_OTHER,
};

use crate::privilege::enable_privilege;

const PLANNED: u32 =
    SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_MINOR_OTHER | SHTDN_REASON_FLAG_PLANNED;

pub fn sign_out() -> io::Result<()> {
    // SAFETY: a plain request; Windows carries it out after this returns.
    if unsafe { ExitWindowsEx(EWX_LOGOFF, PLANNED) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Every account may restart its own PC, but the privilege must be on.
pub fn restart() -> io::Result<()> {
    enable_privilege("SeShutdownPrivilege")?;
    // SAFETY: as above.
    if unsafe { ExitWindowsEx(EWX_REBOOT, PLANNED) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
