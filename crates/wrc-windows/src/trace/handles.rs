//! Names of keys opened before tracing started, from Windows' handle
//! table: the key object an event names is found among every process's
//! open handles, that handle is duplicated into this process, and the key
//! is asked its full name, as Process Explorer does. Needs administrator
//! rights; a protected process's handles cannot be duplicated, so its keys
//! stay unnamed.

use std::collections::{HashMap, HashSet};

use windows_sys::Wdk::System::Registry::{KeyNameInformation, NtQueryKey};
use windows_sys::Wdk::System::SystemInformation::NtQuerySystemInformation;
use windows_sys::Win32::Foundation::{CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, PROCESS_DUP_HANDLE,
};

/// SystemExtendedHandleInformation: every handle with its object address.
const EXTENDED_HANDLES: i32 = 64;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32.cast_signed();
const STATUS_BUFFER_OVERFLOW: i32 = 0x8000_0005_u32.cast_signed();
const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023_u32.cast_signed();
/// The table outgrows its buffer between calls; give up after this many.
const ATTEMPTS: usize = 6;

/// SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX.
#[repr(C)]
#[derive(Clone, Copy)]
struct Entry {
    object: usize,
    pid: usize,
    handle: usize,
    access: u32,
    backtrace: u16,
    kind: u16,
    attributes: u32,
    reserved: u32,
}

/// The full kernel names of these key objects, for those found.
pub fn names(objects: &HashSet<u64>) -> HashMap<u64, String> {
    let mut found = HashMap::new();
    if objects.is_empty() {
        return found;
    }
    // Other accounts' processes (services) need it; held by administrators.
    let _ = crate::privilege::enable_privilege("SeDebugPrivilege");
    let Some(entries) = table() else {
        return found;
    };
    // SAFETY: plain queries of this process.
    let (own, this) = unsafe { (GetCurrentProcessId(), GetCurrentProcess()) };
    let mut processes: HashMap<u32, HANDLE> = HashMap::new();
    for entry in entries {
        let object = entry.object as u64;
        // Windows hides object addresses (all zero) from callers without
        // administrator rights; zero names nothing.
        if object == 0 || !objects.contains(&object) || found.contains_key(&object) {
            continue;
        }
        let Ok(pid) = u32::try_from(entry.pid) else {
            continue;
        };
        let handle = entry.handle as HANDLE;
        let name = if pid == own {
            key_name(handle)
        } else {
            let process = *processes.entry(pid).or_insert_with(|| {
                // SAFETY: a plain open; a refusal returns null, checked below.
                unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, pid) }
            });
            if process.is_null() {
                continue;
            }
            let mut copy: HANDLE = std::ptr::null_mut();
            // SAFETY: both process handles are valid; the copy is closed below.
            let ok = unsafe {
                DuplicateHandle(
                    process,
                    handle,
                    this,
                    &mut copy,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            if ok == 0 {
                continue;
            }
            let name = key_name(copy);
            // SAFETY: the duplicate belongs to this process and is closed once.
            unsafe { CloseHandle(copy) };
            name
        };
        if let Some(name) = name {
            found.insert(object, name);
        }
    }
    for process in processes.into_values().filter(|process| !process.is_null()) {
        // SAFETY: each came from OpenProcess and is closed once.
        unsafe { CloseHandle(process) };
    }
    found
}

/// Every open handle on the machine.
fn table() -> Option<Vec<Entry>> {
    let mut size: usize = 4 << 20;
    for _ in 0..ATTEMPTS {
        let mut buffer = vec![0u64; size.div_ceil(8)];
        let mut needed = 0u32;
        // SAFETY: the buffer holds `size` bytes, 8-byte aligned.
        let status = unsafe {
            NtQuerySystemInformation(
                EXTENDED_HANDLES,
                buffer.as_mut_ptr().cast(),
                u32::try_from(size).ok()?,
                &mut needed,
            )
        };
        if status == STATUS_INFO_LENGTH_MISMATCH {
            size = usize::try_from(needed).ok()?.max(size) + (1 << 20);
            continue;
        }
        if status < 0 {
            return None;
        }
        // SYSTEM_HANDLE_INFORMATION_EX: a count, a reserved word, entries.
        let count = usize::try_from(buffer[0]).ok()?;
        let header = 2 * std::mem::size_of::<usize>();
        let room = (size - header) / std::mem::size_of::<Entry>();
        // SAFETY: Windows wrote `count` entries after the header; the count
        // is capped by what fits in the buffer.
        let entries = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<u8>().add(header).cast::<Entry>(),
                count.min(room),
            )
        };
        return Some(entries.to_vec());
    }
    None
}

/// A key handle's full kernel name (`\REGISTRY\USER\S-1-5-…\Software\…`).
fn key_name(handle: HANDLE) -> Option<String> {
    let mut buffer = vec![0u32; 512];
    loop {
        let mut needed = 0u32;
        let length = u32::try_from(buffer.len() * 4).ok()?;
        // SAFETY: the buffer holds `length` bytes, 4-byte aligned.
        let status = unsafe {
            NtQueryKey(
                handle,
                KeyNameInformation,
                buffer.as_mut_ptr().cast(),
                length,
                &mut needed,
            )
        };
        if (status == STATUS_BUFFER_OVERFLOW || status == STATUS_BUFFER_TOO_SMALL)
            && needed > length
        {
            buffer = vec![0u32; usize::try_from(needed).ok()?.div_ceil(4)];
            continue;
        }
        if status < 0 {
            return None;
        }
        // KEY_NAME_INFORMATION: the name's length in bytes, then the name.
        let bytes = usize::try_from(buffer[0]).ok()?;
        let units = bytes / 2;
        // SAFETY: Windows wrote `bytes` bytes of name after the length.
        let name = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().add(1).cast::<u16>(),
                units.min(buffer.len() * 2 - 2),
            )
        };
        return Some(String::from_utf16_lossy(name));
    }
}

/// How many of this process's handles show their object address, out of
/// how many: none without administrator rights.
pub fn visible() -> (usize, usize) {
    let _ = crate::privilege::enable_privilege("SeDebugPrivilege");
    let own = std::process::id() as usize;
    let mine: Vec<Entry> = table()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.pid == own)
        .collect();
    let shown = mine.iter().filter(|entry| entry.object != 0).count();
    (shown, mine.len())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_key_handle_is_asked_its_full_name() {
        let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey("Software")
            .unwrap();
        let name = super::key_name(key.raw_handle() as _).unwrap();
        assert!(name.starts_with(r"\REGISTRY\USER\S-1-5-"), "{name}");
        assert!(name.ends_with(r"\Software"), "{name}");
    }
}
