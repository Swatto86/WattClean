//! Turning one ETW event into a `Raw` change, by field name through the
//! Trace Data Helper (the field layouts are Windows' own event manifests).

use std::ffi::OsStr;

use windows_sys::Win32::System::Diagnostics::Etw::{
    EVENT_RECORD, PROPERTY_DATA_DESCRIPTOR, TdhGetProperty, TdhGetPropertySize,
};

use crate::privilege::wide;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Process,
    File,
    Registry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileOp {
    Created,
    Overwritten,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RegOp {
    KeyDeleted,
    ValueSet,
    ValueDeleted,
}

/// One change as Windows reports it: kernel paths (`\Device\HarddiskVolume3\…`,
/// `\REGISTRY\MACHINE\…`), not yet the names people use.
#[derive(Debug, Clone)]
pub enum Raw {
    ProcessStart {
        pid: u32,
        parent: u32,
        image: String,
    },
    ProcessStop {
        pid: u32,
    },
    File {
        pid: u32,
        time: i64,
        op: FileOp,
        path: String,
        directory: bool,
    },
    /// A key opened or created; `object` names it in later events, which
    /// often carry no key path of their own.
    KeyOpened {
        pid: u32,
        time: i64,
        object: u64,
        base: u64,
        base_name: String,
        relative: String,
        created: bool,
    },
    Registry {
        pid: u32,
        time: i64,
        op: RegOp,
        object: u64,
        key: String,
        value: Option<String>,
        /// The value had data before this set: a change, not a creation.
        existed: bool,
    },
}

/// NtCreateFile dispositions that replace an existing file's contents.
const SUPERSEDE: u32 = 0;
const OVERWRITE: u32 = 4;
const OVERWRITE_IF: u32 = 5;
const DIRECTORY_FILE: u32 = 0x1;
/// REG_CREATED_NEW_KEY: CreateKey made the key rather than opening an existing one.
const CREATED_NEW_KEY: u32 = 1;

pub fn decode(record: &EVENT_RECORD, provider: Provider, id: u16, pid: u32) -> Option<Raw> {
    match (provider, id) {
        (Provider::Process, 1) => Some(Raw::ProcessStart {
            pid: number(record, "ProcessID")?,
            parent: number(record, "ParentProcessID")?,
            image: text(record, "ImageName").unwrap_or_default(),
        }),
        (Provider::Process, 2) => Some(Raw::ProcessStop {
            pid: number(record, "ProcessID")?,
        }),
        (Provider::File, 12 | 30) => {
            let options = number(record, "CreateOptions")?;
            let disposition = options >> 24;
            let op = if id == 30 {
                FileOp::Created
            } else if [SUPERSEDE, OVERWRITE, OVERWRITE_IF].contains(&disposition) {
                FileOp::Overwritten
            } else {
                return None;
            };
            let directory = options & DIRECTORY_FILE != 0;
            Some(Raw::File {
                pid,
                time: record.EventHeader.TimeStamp,
                op,
                path: text(record, "FileName")?,
                directory,
            })
        }
        (Provider::File, 26 | 27) => {
            let op = if id == 26 {
                FileOp::Deleted
            } else {
                FileOp::Renamed
            };
            Some(Raw::File {
                pid,
                time: record.EventHeader.TimeStamp,
                op,
                path: text(record, "FilePath")?,
                directory: false,
            })
        }
        (Provider::Registry, 1 | 2) => {
            if number(record, "Status")? != 0 {
                return None;
            }
            Some(Raw::KeyOpened {
                pid,
                time: record.EventHeader.TimeStamp,
                object: pointer(record, "KeyObject")?,
                base: pointer(record, "BaseObject").unwrap_or(0),
                base_name: text(record, "BaseName").unwrap_or_default(),
                relative: text(record, "RelativeName").unwrap_or_default(),
                created: id == 1 && number(record, "Disposition") == Some(CREATED_NEW_KEY),
            })
        }
        (Provider::Registry, 3 | 5 | 6) => {
            if number(record, "Status")? != 0 {
                return None;
            }
            let (op, value) = match id {
                3 => (RegOp::KeyDeleted, None),
                5 => (
                    RegOp::ValueSet,
                    Some(text(record, "ValueName").unwrap_or_default()),
                ),
                _ => (
                    RegOp::ValueDeleted,
                    Some(text(record, "ValueName").unwrap_or_default()),
                ),
            };
            Some(Raw::Registry {
                pid,
                time: record.EventHeader.TimeStamp,
                op,
                object: pointer(record, "KeyObject")?,
                key: text(record, "KeyName").unwrap_or_default(),
                value,
                // Kernel-Registry reports the value's previous type and size;
                // both zero means it did not exist.
                existed: id == 5
                    && (number(record, "PreviousDataType").unwrap_or(0) != 0
                        || number(record, "PreviousDataSize").unwrap_or(0) != 0),
            })
        }
        _ => None,
    }
}

/// The raw bytes of one named field, or `None` when the event lacks it.
fn property(record: &EVENT_RECORD, name: &str) -> Option<Vec<u8>> {
    let name = wide(OsStr::new(name));
    let descriptor = PROPERTY_DATA_DESCRIPTOR {
        PropertyName: u64::try_from(name.as_ptr() as usize).ok()?,
        ArrayIndex: u32::MAX,
        Reserved: 0,
    };
    let mut size = 0u32;
    // SAFETY: `record` is the live event; the descriptor names a
    // NUL-terminated string that outlives both calls.
    let status =
        unsafe { TdhGetPropertySize(record, 0, std::ptr::null(), 1, &descriptor, &mut size) };
    if status != 0 {
        return None;
    }
    let mut buffer = vec![0u8; usize::try_from(size).ok()?];
    // SAFETY: as above, into a buffer of exactly the reported size.
    let status = unsafe {
        TdhGetProperty(
            record,
            0,
            std::ptr::null(),
            1,
            &descriptor,
            size,
            buffer.as_mut_ptr(),
        )
    };
    (status == 0).then_some(buffer)
}

fn number(record: &EVENT_RECORD, name: &str) -> Option<u32> {
    let bytes = property(record, name)?;
    match bytes.as_slice() {
        [a, b, c, d] => Some(u32::from_le_bytes([*a, *b, *c, *d])),
        [a, b, c, d, e, f, g, h] => {
            u32::try_from(u64::from_le_bytes([*a, *b, *c, *d, *e, *f, *g, *h])).ok()
        }
        _ => None,
    }
}

fn pointer(record: &EVENT_RECORD, name: &str) -> Option<u64> {
    let bytes = property(record, name)?;
    match bytes.as_slice() {
        [a, b, c, d] => Some(u64::from(u32::from_le_bytes([*a, *b, *c, *d]))),
        [a, b, c, d, e, f, g, h] => Some(u64::from_le_bytes([*a, *b, *c, *d, *e, *f, *g, *h])),
        _ => None,
    }
}

fn text(record: &EVENT_RECORD, name: &str) -> Option<String> {
    let bytes = property(record, name)?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|&unit| unit != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}
