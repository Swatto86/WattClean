//! Owners and permissions of keys: read them as SDDL text, put them back,
//! and take a key over. Taking over a key someone else owns needs
//! SeTakeOwnershipPrivilege, and giving a key back to another owner (such
//! as TrustedInstaller) needs SeRestorePrivilege; administrators hold both,
//! switched off until asked for.

use std::ffi::OsStr;
use std::io;

use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};
use windows_sys::Win32::System::Registry::{RegGetKeySecurity, RegSetKeySecurity};
use winreg::RegKey;
use winreg::enums::{KEY_ENUMERATE_SUB_KEYS, KEY_WOW64_64KEY};

use crate::privilege::{enable_privilege, wide};

const READ_CONTROL: u32 = 0x0002_0000;
const WRITE_DAC: u32 = 0x0004_0000;
const WRITE_OWNER: u32 = 0x0008_0000;
const ALL_PARTS: u32 =
    OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;

/// Switch on the two privileges; a process without them (not elevated)
/// simply goes on without, and Windows refuses what needs them.
fn privileges() {
    let _ = enable_privilege("SeTakeOwnershipPrivilege");
    let _ = enable_privilege("SeRestorePrivilege");
}

fn status(code: u32) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            i32::try_from(code).unwrap_or(-1),
        ))
    }
}

/// Owner, group and permissions of `path` under `root`, as SDDL.
pub fn read(root: &RegKey, path: &str) -> io::Result<String> {
    let key = root.open_subkey_with_flags(path, READ_CONTROL | KEY_WOW64_64KEY)?;
    let handle = key.raw_handle() as _;
    let mut size = 0u32;
    // SAFETY: a null buffer with a size of zero asks for the size needed.
    let first = unsafe { RegGetKeySecurity(handle, ALL_PARTS, std::ptr::null_mut(), &mut size) };
    if first != 0 && first != ERROR_INSUFFICIENT_BUFFER {
        status(first)?;
    }
    // u64 units keep the descriptor aligned.
    let mut buffer = vec![0u64; usize::try_from(size).unwrap_or(0).div_ceil(8)];
    // SAFETY: the buffer is at least `size` bytes and outlives the call.
    status(unsafe { RegGetKeySecurity(handle, ALL_PARTS, buffer.as_mut_ptr().cast(), &mut size) })?;
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: `buffer` holds the descriptor just read; Windows allocates `text`.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            buffer.as_mut_ptr().cast(),
            SDDL_REVISION_1,
            ALL_PARTS,
            &mut text,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 || text.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `text` is a NUL-terminated string Windows allocated.
    let length = (0..)
        .take_while(|&at| unsafe { *text.add(at) } != 0)
        .count();
    // SAFETY: `length` units were just read from it.
    let sddl = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
    // SAFETY: allocated by the conversion above, released once.
    unsafe { LocalFree(text.cast()) };
    Ok(sddl)
}

/// A security descriptor built from SDDL, released when dropped.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let text = wide(OsStr::new(sddl));
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `text` is NUL-terminated and outlives the call; Windows
        // allocates the descriptor, which `Drop` releases.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || descriptor.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(self.0) };
    }
}

/// Set the parts `sddl` names (owner `O:`, group `G:`, permissions `D:`).
pub fn write(root: &RegKey, path: &str, sddl: &str) -> io::Result<()> {
    privileges();
    let mut parts = 0;
    let mut access = 0;
    if sddl.contains("O:") {
        parts |= OWNER_SECURITY_INFORMATION;
        access |= WRITE_OWNER;
    }
    if sddl.contains("G:") {
        parts |= GROUP_SECURITY_INFORMATION;
        access |= WRITE_OWNER;
    }
    if sddl.contains("D:") {
        parts |= DACL_SECURITY_INFORMATION;
        access |= WRITE_DAC;
    }
    let descriptor = Descriptor::from_sddl(sddl)?;
    let key = root
        .open_subkey_with_flags(path, access | KEY_WOW64_64KEY)
        .map_err(|error| context(error, &format!("opening {path} to change its security")))?;
    // SAFETY: the key is open with the access these parts need; the
    // descriptor is valid until `descriptor` drops after the call.
    status(unsafe { RegSetKeySecurity(key.raw_handle() as _, parts, descriptor.0) })
        .map_err(|error| context(error, &format!("changing the security of {path}")))
}

/// The error with what was being done, keeping its kind.
fn context(error: io::Error, doing: &str) -> io::Error {
    io::Error::new(error.kind(), format!("{doing}: {error}"))
}

/// Make `user_sid` the owner with full control (replacing the
/// permissions), for the key and, with `subkeys`, everything beneath it.
pub fn take(root: &RegKey, path: &str, user_sid: &str, subkeys: bool) -> io::Result<()> {
    write(root, path, &format!("O:{user_sid}"))?;
    write(
        root,
        path,
        &format!("D:P(A;OICI;KA;;;{user_sid})(A;OICI;KA;;;BA)(A;OICI;KA;;;SY)"),
    )?;
    if subkeys {
        let key = root
            .open_subkey_with_flags(path, KEY_ENUMERATE_SUB_KEYS | KEY_WOW64_64KEY)
            .map_err(|error| context(error, &format!("listing the subkeys of {path}")))?;
        for name in key.enum_keys() {
            take(root, &format!(r"{path}\{}", name?), user_sid, true)?;
        }
    }
    Ok(())
}
