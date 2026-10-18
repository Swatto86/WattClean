//! Prototype: watch what an installer does through Event Tracing for Windows,
//! with no driver. A private real-time session listens to Windows' own
//! kernel providers for processes, files and the registry; only events from
//! the installer's process tree are kept. Needs administrator rights.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::io;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use windows_sys::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW, EVENT_CONTROL_CODE_ENABLE_PROVIDER,
    EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2, OpenTraceW, PROCESS_TRACE_MODE_EVENT_RECORD,
    PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE, ProcessTrace, StartTraceW,
    WNODE_FLAG_TRACED_GUID,
};
use windows_sys::core::GUID;

use crate::privilege::wide;
use crate::trace_event::{Provider, Raw, decode};

const PROCESS: GUID = GUID::from_u128(0x22fb2cd6_0e7b_422b_a0c7_2fad1fd0e716);
const FILE: GUID = GUID::from_u128(0xedd08927_9cc4_4e65_b970_c2560fb5c289);
const REGISTRY: GUID = GUID::from_u128(0x70eb4f03_c1de_4f73_a051_33d13d5413bd);
/// Process start/stop; file create, delete, rename, new file; registry
/// create and open key (to name keys), delete key, set value, delete value.
const PROVIDERS: [(GUID, u64); 3] = [
    (PROCESS, 0x10),
    (FILE, 0x80 | 0x400 | 0x800 | 0x1000),
    (REGISTRY, 0x1000 | 0x2000 | 0x4000 | 0x100 | 0x200),
];
const ERROR_ALREADY_EXISTS: u32 = 183;
/// TRACE_LEVEL_VERBOSE: every event the keywords select.
const LEVEL: u8 = 5;

/// Which processes belong to the traced install.
#[derive(Default)]
pub struct Tracker {
    tracked: HashSet<u32>,
    live: HashSet<u32>,
    /// Also follow Windows Installer processes started while tracing: MSI
    /// work runs in the Windows Installer service, not under the installer.
    pub follow_msiexec: bool,
}

impl Tracker {
    pub fn is_tracked(&self, pid: u32) -> bool {
        self.tracked.contains(&pid)
    }

    fn start(&mut self, pid: u32, parent: u32, image: &str) -> bool {
        let msiexec = self.follow_msiexec && image.to_ascii_lowercase().ends_with("\\msiexec.exe");
        let follow = self.tracked.contains(&parent) || msiexec;
        if follow {
            self.tracked.insert(pid);
            self.live.insert(pid);
        }
        follow
    }
}

struct Context {
    tracker: Arc<Mutex<Tracker>>,
    sink: Sender<Raw>,
}

fn lock(tracker: &Mutex<Tracker>) -> MutexGuard<'_, Tracker> {
    tracker.lock().unwrap_or_else(PoisonError::into_inner)
}

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    // SAFETY: ETW passes a record valid for the duration of this call.
    let Some(record) = (unsafe { record.as_ref() }) else {
        return;
    };
    // SAFETY: UserContext is the `Context` that `Session::start` leaked; it is
    // freed only after ProcessTrace returns, so no callback outlives it.
    let context = unsafe { &*record.UserContext.cast::<Context>() };
    let header = &record.EventHeader;
    let id = header.EventDescriptor.Id;
    let provider = if guid_eq(&header.ProviderId, &PROCESS) {
        Provider::Process
    } else if guid_eq(&header.ProviderId, &FILE) {
        Provider::File
    } else {
        Provider::Registry
    };
    if provider == Provider::Process {
        on_process(record, context, id);
        return;
    }
    if !lock(&context.tracker).is_tracked(header.ProcessId) {
        return;
    }
    if let Some(event) = decode(record, provider, id, header.ProcessId) {
        // A closed receiver only means the caller stopped listening.
        let _ = context.sink.send(event);
    }
}

fn on_process(record: &EVENT_RECORD, context: &Context, id: u16) {
    match decode(record, Provider::Process, id, 0) {
        Some(Raw::ProcessStart { pid, parent, image })
            if lock(&context.tracker).start(pid, parent, &image) =>
        {
            let _ = context.sink.send(Raw::ProcessStart { pid, parent, image });
        }
        Some(Raw::ProcessStop { pid }) if lock(&context.tracker).live.remove(&pid) => {
            let _ = context.sink.send(Raw::ProcessStop { pid });
        }
        _ => {}
    }
}
fn guid_eq(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

/// Counters from the stopped session: anything above zero means events were missed.
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub events_lost: u32,
    pub buffers_lost: u32,
}

pub struct Session {
    name: Vec<u16>,
    control: CONTROLTRACE_HANDLE,
    properties: Vec<u64>,
    thread: Option<JoinHandle<()>>,
    tracker: Arc<Mutex<Tracker>>,
}

impl Session {
    /// Start a private session and its processing thread.
    pub fn start(follow_msiexec: bool) -> io::Result<(Self, Receiver<Raw>)> {
        let name = wide(OsStr::new(&format!(
            "WattClean-Trace-{}",
            std::process::id()
        )));
        let mut properties = properties_buffer(&name);
        let mut control = CONTROLTRACE_HANDLE { Value: 0 };
        // SAFETY: `properties` is an aligned buffer holding EVENT_TRACE_PROPERTIES
        // followed by room for the session name, as StartTraceW requires.
        let mut status =
            unsafe { StartTraceW(&mut control, name.as_ptr(), properties.as_mut_ptr().cast()) };
        if status == ERROR_ALREADY_EXISTS {
            stop_by_name(&name);
            properties = properties_buffer(&name);
            // SAFETY: as above, after the stale session was stopped.
            status =
                unsafe { StartTraceW(&mut control, name.as_ptr(), properties.as_mut_ptr().cast()) };
        }
        check(status)?;
        for (provider, keywords) in PROVIDERS {
            // SAFETY: the session handle is live and the GUID outlives the call.
            let enabled = unsafe {
                EnableTraceEx2(
                    control,
                    &provider,
                    EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                    LEVEL,
                    keywords,
                    0,
                    0,
                    std::ptr::null(),
                )
            };
            if let Err(error) = check(enabled) {
                stop_by_name(&name);
                return Err(error);
            }
        }
        let tracker = Arc::new(Mutex::new(Tracker {
            follow_msiexec,
            ..Tracker::default()
        }));
        let (sink, events) = channel();
        let context = Box::into_raw(Box::new(Context {
            tracker: Arc::clone(&tracker),
            sink,
        }));
        let thread = match open_and_process(&name, context) {
            Ok(thread) => thread,
            Err(error) => {
                stop_by_name(&name);
                // SAFETY: the processing thread never started, so this is the only owner.
                drop(unsafe { Box::from_raw(context) });
                return Err(error);
            }
        };
        Ok((
            Self {
                name,
                control,
                properties,
                thread: Some(thread),
                tracker,
            },
            events,
        ))
    }

    pub fn track(&self, pid: u32) {
        let mut tracker = lock(&self.tracker);
        tracker.tracked.insert(pid);
        tracker.live.insert(pid);
    }

    /// Tracked processes not yet seen to exit.
    pub fn live(&self) -> usize {
        lock(&self.tracker).live.len()
    }

    pub fn stop(mut self) -> io::Result<Stats> {
        // SAFETY: the handle and properties buffer belong to this live session.
        let status = unsafe {
            ControlTraceW(
                self.control,
                self.name.as_ptr(),
                self.properties.as_mut_ptr().cast(),
                EVENT_TRACE_CONTROL_STOP,
            )
        };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        check(status)?;
        // SAFETY: ControlTraceW filled the buffer with the final counters.
        let filled = unsafe { &*self.properties.as_ptr().cast::<EVENT_TRACE_PROPERTIES>() };
        Ok(Stats {
            events_lost: filled.EventsLost,
            buffers_lost: filled.RealTimeBuffersLost + filled.LogBuffersLost,
        })
    }
}

impl Drop for Session {
    /// A session left running would keep tracing until Windows restarts.
    fn drop(&mut self) {
        if self.thread.is_some() {
            stop_by_name(&self.name);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

fn check(status: u32) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(-1),
        ))
    }
}

/// EVENT_TRACE_PROPERTIES followed by space for the name, in u64s for alignment.
fn properties_buffer(name: &[u16]) -> Vec<u64> {
    let header = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
    let bytes = header + name.len() * 2 + 1024;
    let mut buffer = vec![0u64; bytes.div_ceil(8)];
    // SAFETY: the buffer is zeroed, 8-byte aligned and larger than the structure.
    let properties = unsafe { &mut *buffer.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>() };
    properties.Wnode.BufferSize = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
    properties.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
    properties.Wnode.ClientContext = 1;
    properties.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
    properties.BufferSize = 256;
    properties.MinimumBuffers = 64;
    properties.MaximumBuffers = 512;
    properties.FlushTimer = 1;
    properties.LoggerNameOffset = u32::try_from(header).unwrap_or(0);
    buffer
}

fn stop_by_name(name: &[u16]) {
    let mut properties = properties_buffer(name);
    // SAFETY: stopping by name needs only a valid properties buffer.
    unsafe {
        ControlTraceW(
            CONTROLTRACE_HANDLE { Value: 0 },
            name.as_ptr(),
            properties.as_mut_ptr().cast(),
            EVENT_TRACE_CONTROL_STOP,
        )
    };
}

fn open_and_process(name: &[u16], context: *mut Context) -> io::Result<JoinHandle<()>> {
    let mut logger = name.to_vec();
    // SAFETY: an all-zero EVENT_TRACE_LOGFILEW is a valid "unset" value.
    let mut logfile: EVENT_TRACE_LOGFILEW = unsafe { std::mem::zeroed() };
    logfile.LoggerName = logger.as_mut_ptr();
    logfile.Anonymous1.ProcessTraceMode =
        PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
    logfile.Anonymous2.EventRecordCallback = Some(on_event);
    logfile.Context = context.cast();
    // SAFETY: `logfile` and the name it points to are valid for this call.
    let handle = unsafe { OpenTraceW(&mut logfile) };
    if handle.Value == u64::MAX {
        return Err(io::Error::last_os_error());
    }
    let context = context as usize;
    Ok(std::thread::spawn(move || {
        let handles = [PROCESSTRACE_HANDLE {
            Value: handle.Value,
        }];
        // SAFETY: the handle came from OpenTraceW; ProcessTrace returns once
        // the session stops, after which no callback can run.
        unsafe {
            ProcessTrace(handles.as_ptr(), 1, std::ptr::null(), std::ptr::null());
            CloseTrace(handles[0]);
            drop(Box::from_raw(context as *mut Context));
        }
    }))
}

/// Drive letters and the kernel device each maps to (`C:` →
/// `\Device\HarddiskVolume3`), for turning event paths into normal ones.
pub fn volumes() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:", char::from(letter));
        let name = wide(OsStr::new(&drive));
        let mut buffer = vec![0u16; 1024];
        // SAFETY: `name` is NUL-terminated and the buffer length is passed exactly.
        let written = unsafe {
            windows_sys::Win32::Storage::FileSystem::QueryDosDeviceW(
                name.as_ptr(),
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(0),
            )
        };
        if written == 0 {
            continue;
        }
        let device: Vec<u16> = buffer
            .iter()
            .copied()
            .take_while(|&unit| unit != 0)
            .collect();
        found.push((String::from_utf16_lossy(&device), drive));
    }
    found
}
