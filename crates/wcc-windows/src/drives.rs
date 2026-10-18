//! Fixed and removable drives with their free space, for wiping free space.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::ptr::null_mut;

use wcc_core::system::Drive;
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};

use crate::win::{total_space, wide};

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

fn volume(root: &str) -> (String, String) {
    let path = wide(root);
    let mut label = vec![0u16; 261];
    let mut file_system = vec![0u16; 261];
    // SAFETY: both buffers are 261 characters, as passed; others are null.
    let ok = unsafe {
        GetVolumeInformationW(
            path.as_ptr(),
            label.as_mut_ptr(),
            261,
            null_mut(),
            null_mut(),
            null_mut(),
            file_system.as_mut_ptr(),
            261,
        )
    };
    let text = |buffer: &[u16]| {
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        OsString::from_wide(&buffer[..len])
            .to_string_lossy()
            .to_string()
    };
    if ok == 0 {
        (String::new(), String::new())
    } else {
        (text(&label), text(&file_system))
    }
}

pub fn list() -> Vec<Drive> {
    let mut buffer = vec![0u16; 512];
    // SAFETY: the buffer holds 512 characters, the length passed.
    let len = unsafe { GetLogicalDriveStringsW(512, buffer.as_mut_ptr()) };
    let len = usize::try_from(len).unwrap_or(0).min(buffer.len());
    buffer[..len]
        .split(|&c| c == 0)
        .filter(|root| !root.is_empty())
        .filter_map(|root| {
            let root = OsString::from_wide(root).to_string_lossy().to_string();
            let path = wide(&root);
            // SAFETY: a NUL-terminated root path such as `C:\`.
            let kind = unsafe { GetDriveTypeW(path.as_ptr()) };
            if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
                return None;
            }
            let (free, total) = total_space(&root)?;
            let (label, file_system) = volume(&root);
            Some(Drive {
                root,
                label,
                file_system,
                free,
                total,
                removable: kind == DRIVE_REMOVABLE,
            })
        })
        .collect()
}
