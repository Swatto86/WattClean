//! One ETW event as a `Raw`. Registry events come thousands a second, so
//! their fields are read straight from the event's data in the order
//! Windows' Kernel-Registry manifest declares them (newer versions only add
//! fields at the end); the rare process events go through the Trace Data
//! Helper by field name.

use std::ffi::OsStr;

use windows_sys::Win32::System::Diagnostics::Etw::{
    EVENT_HEADER_FLAG_32_BIT_HEADER, EVENT_RECORD, PROPERTY_DATA_DESCRIPTOR, TdhGetProperty,
    TdhGetPropertySize,
};
use wrc_core::trace::{Captured, WriteOp};

use crate::privilege::wide;

/// One event as Windows reports it, with kernel key names
/// (`\REGISTRY\MACHINE\…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raw {
    ProcessStart {
        pid: u32,
        image: String,
    },
    ProcessStop {
        pid: u32,
    },
    KeyOpened {
        pid: u32,
        object: u64,
        base: u64,
        base_name: String,
        relative: String,
        created: bool,
    },
    KeyClosed {
        object: u64,
    },
    Registry {
        pid: u32,
        op: WriteOp,
        object: u64,
        /// Usually empty: the key is named by `object`.
        key: String,
        value: Option<String>,
        /// A value set: the data written, and what it replaced.
        new: Option<Captured>,
        old: Option<Captured>,
    },
}

/// Kernel-Registry event ids.
const CREATE_KEY: u16 = 1;
const OPEN_KEY: u16 = 2;
const DELETE_KEY: u16 = 3;
const SET_VALUE: u16 = 5;
const DELETE_VALUE: u16 = 6;
const CLOSE_KEY: u16 = 13;
/// REG_CREATED_NEW_KEY: CreateKey made the key rather than opening it.
const CREATED_NEW_KEY: u32 = 1;

/// The event's own data, read field by field.
struct Fields<'a> {
    data: &'a [u8],
    at: usize,
    pointer: usize,
}

impl<'a> Fields<'a> {
    fn new(record: &'a EVENT_RECORD) -> Self {
        let data = if record.UserData.is_null() {
            &[][..]
        } else {
            // SAFETY: ETW guarantees UserData holds UserDataLength bytes for
            // the duration of the callback, which outlives `Fields`.
            unsafe {
                std::slice::from_raw_parts(
                    record.UserData.cast::<u8>(),
                    usize::from(record.UserDataLength),
                )
            }
        };
        let narrow = u32::from(record.EventHeader.Flags) & EVENT_HEADER_FLAG_32_BIT_HEADER != 0;
        Self {
            data,
            at: 0,
            pointer: if narrow { 4 } else { 8 },
        }
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let bytes = self.data.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        let bytes = self.take(4)?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    }

    fn u16(&mut self) -> Option<u16> {
        let bytes = self.take(2)?;
        Some(u16::from_le_bytes(bytes.try_into().ok()?))
    }

    /// Captured data: its length, then that many bytes. `None` when Windows
    /// passed none, as it does unless asked to capture data: the data is
    /// then unknown, not empty.
    fn captured(&mut self, kind: u32, size: u32) -> Option<Option<Captured>> {
        let length = self.u16()?;
        let bytes = self.take(usize::from(length))?.to_vec();
        Some((length > 0 || size == 0).then_some(Captured { kind, size, bytes }))
    }

    fn pointer(&mut self) -> Option<u64> {
        let bytes = self.take(self.pointer)?;
        Some(match bytes.len() {
            4 => u64::from(u32::from_le_bytes(bytes.try_into().ok()?)),
            _ => u64::from_le_bytes(bytes.try_into().ok()?),
        })
    }

    /// A NUL-terminated UTF-16 string.
    fn text(&mut self) -> Option<String> {
        let rest = self.data.get(self.at..)?;
        let mut units = Vec::new();
        for pair in rest.as_chunks::<2>().0 {
            self.at += 2;
            let unit = u16::from_le_bytes(*pair);
            if unit == 0 {
                return Some(String::from_utf16_lossy(&units));
            }
            units.push(unit);
        }
        None
    }
}

/// A Kernel-Registry event, or `None` for a failed operation or one not
/// followed.
pub fn registry(record: &EVENT_RECORD) -> Option<Raw> {
    let pid = record.EventHeader.ProcessId;
    let mut fields = Fields::new(record);
    match record.EventHeader.EventDescriptor.Id {
        id @ (CREATE_KEY | OPEN_KEY) => {
            let base = fields.pointer()?;
            let object = fields.pointer()?;
            let status = fields.u32()?;
            let disposition = fields.u32()?;
            if status != 0 {
                return None;
            }
            Some(Raw::KeyOpened {
                pid,
                object,
                base,
                base_name: fields.text()?,
                relative: fields.text()?,
                created: id == CREATE_KEY && disposition == CREATED_NEW_KEY,
            })
        }
        CLOSE_KEY => Some(Raw::KeyClosed {
            object: fields.pointer()?,
        }),
        id @ (DELETE_KEY | SET_VALUE | DELETE_VALUE) => {
            let object = fields.pointer()?;
            if fields.u32()? != 0 {
                return None;
            }
            let written = if id == SET_VALUE {
                Some((fields.u32()?, fields.u32()?))
            } else {
                None
            };
            let key = fields.text()?;
            let value = (id != DELETE_KEY).then(|| fields.text()).flatten();
            let op = match id {
                DELETE_KEY => WriteOp::KeyDeleted,
                SET_VALUE => WriteOp::ValueSet,
                _ => WriteOp::ValueDeleted,
            };
            let (new, old) = match written {
                Some((kind, size)) => data(&mut fields, kind, size),
                None => (None, None),
            };
            Some(Raw::Registry {
                pid,
                op,
                object,
                key,
                value,
                new,
                old,
            })
        }
        _ => None,
    }
}

/// A SetValueKey event's data: what was written, and the previous data
/// (none when the value did not exist: its type and size are both zero).
/// Missing fields leave the data unknown rather than the write unseen.
fn data(fields: &mut Fields<'_>, kind: u32, size: u32) -> (Option<Captured>, Option<Captured>) {
    let Some(new) = fields.captured(kind, size) else {
        return (None, None);
    };
    let old = (|| {
        let kind = fields.u32()?;
        let size = fields.u32()?;
        let old = fields.captured(kind, size)?;
        if kind == 0 && size == 0 { None } else { old }
    })();
    (new, old)
}

/// A Kernel-Process start (1) or stop (2) event.
pub fn process(record: &EVENT_RECORD) -> Option<Raw> {
    match record.EventHeader.EventDescriptor.Id {
        1 => Some(Raw::ProcessStart {
            pid: number(record, "ProcessID")?,
            image: text(record, "ImageName").unwrap_or_default(),
        }),
        2 => Some(Raw::ProcessStop {
            pid: number(record, "ProcessID")?,
        }),
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
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
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
