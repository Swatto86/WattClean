//! Which process wrote to the registry, through Event Tracing for Windows:
//! a private real-time session on Windows' own Kernel-Registry and
//! Kernel-Process providers, with no driver. Needs administrator rights.
//! Keys being opened and closed are traced too, across the whole machine,
//! to name the keys later events refer to; that costs a little processor
//! time while anything is watched.

mod event;
mod handles;
mod names;
#[cfg(test)]
mod tests;
mod tracker;

use std::ffi::OsStr;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW, EVENT_CONTROL_CODE_ENABLE_PROVIDER,
    EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2, OpenTraceW, PROCESS_TRACE_MODE_EVENT_RECORD,
    PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE, ProcessTrace, StartTraceW,
    WNODE_FLAG_TRACED_GUID,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::core::GUID;
use wrc_core::model::RegPath;
use wrc_core::trace::{RegistryTrace, Write};

use crate::privilege::wide;
use tracker::Tracker;

const PROCESS: GUID = GUID::from_u128(0x22fb2cd6_0e7b_422b_a0c7_2fad1fd0e716);
const REGISTRY: GUID = GUID::from_u128(0x70eb4f03_c1de_4f73_a051_33d13d5413bd);
/// Process start and stop; registry create, open and close key (to name
/// keys), delete key, set value and delete value.
const PROVIDERS: [(GUID, u64); 2] = [
    (PROCESS, 0x10),
    (REGISTRY, 0x1000 | 0x2000 | 0x1 | 0x4000 | 0x100 | 0x200),
];
/// One fixed name, so a session left by a crash is replaced, not doubled.
const SESSION: &str = "WattClean-Registry";
const ERROR_ALREADY_EXISTS: u32 = 183;
/// TRACE_LEVEL_VERBOSE: every event the keywords select.
const LEVEL: u8 = 5;
/// evntrace.h: `FlushTimer` is in milliseconds.
const EVENT_TRACE_USE_MS_FLUSH_TIMER: u32 = 0x10;
/// Windows hands over partly filled buffers this often, well within
/// `tracker::REORDER`.
const FLUSH_MS: u32 = 100;
/// How long `writes` waits for Windows to deliver events.
const WAIT: Duration = Duration::from_secs(1);

fn lock(tracker: &Mutex<Tracker>) -> MutexGuard<'_, Tracker> {
    // Every update leaves the tracker consistent.
    tracker.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct RegistryTracer {
    tracker: Arc<Mutex<Tracker>>,
    /// Kept for its `Drop`, which stops Windows' session.
    _session: Session,
}

/// Start the session and its processing thread.
pub fn start(user_sid: Option<String>) -> io::Result<RegistryTracer> {
    let tracker = Arc::new(Mutex::new(Tracker::new(user_sid)));
    let session = Session::start(Arc::clone(&tracker))?;
    Ok(RegistryTracer {
        tracker,
        _session: session,
    })
}

impl RegistryTrace for RegistryTracer {
    fn follow(&self, keys: &[RegPath]) {
        lock(&self.tracker).follow(keys);
    }

    fn writes(&self, from: u64, to: u64) -> Vec<Write> {
        let deadline = Instant::now() + WAIT;
        loop {
            let mut tracker = lock(&self.tracker);
            let settled = tracker.settled(to);
            if settled || Instant::now() >= deadline {
                if !settled {
                    // Nothing newer arrived to settle the order: take what came.
                    tracker.apply_all(&image_of);
                }
                let asked = tracker.unnamed(from, to);
                if asked.is_empty() {
                    return tracker.writes(from, to);
                }
                // Reading the handle table takes a moment: not under the lock.
                drop(tracker);
                let found = handles::names(&asked);
                let mut tracker = lock(&self.tracker);
                tracker.named(&asked, &found);
                return tracker.writes(from, to);
            }
            drop(tracker);
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

/// For checks: how many of this process's handles Windows' handle table
/// shows with their object address, out of how many.
pub fn handle_addresses() -> (usize, usize) {
    handles::visible()
}

/// The image path of a running process, for one started before tracing.
fn image_of(pid: u32) -> Option<String> {
    // SAFETY: a plain query; the handle is checked and closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 1024];
    let mut size = u32::try_from(buffer.len()).unwrap_or(0);
    // SAFETY: the buffer holds `size` units; the call writes at most that.
    let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size) };
    // SAFETY: `process` came from OpenProcess and is closed once.
    unsafe { CloseHandle(process) };
    let size = usize::try_from(size).ok()?;
    (ok != 0).then(|| String::from_utf16_lossy(buffer.get(..size).unwrap_or_default()))
}

struct Context {
    tracker: Arc<Mutex<Tracker>>,
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
    let raw = if guid_eq(&header.ProviderId, &REGISTRY) {
        event::registry(record)
    } else if guid_eq(&header.ProviderId, &PROCESS) {
        event::process(record)
    } else {
        None
    };
    if let Some(raw) = raw {
        let received = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .map_or(header.TimeStamp, tracker::from_millis);
        lock(&context.tracker).push(header.TimeStamp, received, raw, &image_of);
    }
}

fn guid_eq(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

struct Session {
    name: Vec<u16>,
    thread: Option<JoinHandle<()>>,
}

impl Session {
    fn start(tracker: Arc<Mutex<Tracker>>) -> io::Result<Self> {
        let name = wide(OsStr::new(SESSION));
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
        let context = Box::into_raw(Box::new(Context { tracker }));
        match open_and_process(&name, context) {
            Ok(thread) => Ok(Self {
                name,
                thread: Some(thread),
            }),
            Err(error) => {
                stop_by_name(&name);
                // SAFETY: the processing thread never started, so this is the only owner.
                drop(unsafe { Box::from_raw(context) });
                Err(error)
            }
        }
    }
}

impl Drop for Session {
    /// A session left running would keep tracing until Windows restarts.
    fn drop(&mut self) {
        stop_by_name(&self.name);
        if let Some(thread) = self.thread.take() {
            // ProcessTrace returns once the session stops.
            let _ = thread.join();
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

/// EVENT_TRACE_PROPERTIES followed by space for the name, in u64s for
/// alignment. Small buffers: the session may run for hours.
fn properties_buffer(name: &[u16]) -> Vec<u64> {
    let header = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
    let bytes = header + name.len() * 2 + 1024;
    let mut buffer = vec![0u64; bytes.div_ceil(8)];
    // SAFETY: the buffer is zeroed, 8-byte aligned and larger than the structure.
    let properties = unsafe { &mut *buffer.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>() };
    properties.Wnode.BufferSize = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
    properties.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
    // Precise (QPC) timestamps, delivered as FILETIMEs.
    properties.Wnode.ClientContext = 1;
    properties.LogFileMode = EVENT_TRACE_REAL_TIME_MODE | EVENT_TRACE_USE_MS_FLUSH_TIMER;
    properties.BufferSize = 64;
    properties.MinimumBuffers = 4;
    properties.MaximumBuffers = 64;
    properties.FlushTimer = FLUSH_MS;
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
