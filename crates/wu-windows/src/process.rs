//! Running a program's uninstaller and knowing when it is really done.
//!
//! Many uninstallers copy themselves to a temporary folder, start the copy
//! and exit at once (NSIS, Inno Setup). Waiting for the first process alone
//! would scan for leftovers while the real uninstall is still running, so the
//! uninstaller is put in a job object: every process it starts joins the
//! job, and the uninstall is finished when the job has no live processes.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JobObjectBasicAccountingInformation, QueryInformationJobObject,
};
use wu_core::{Launch, Running};

/// An owned job object handle, closed on drop. Closing it does not end the
/// processes in it: an uninstaller keeps running if this app closes.
struct Job(HANDLE);

impl Job {
    fn new() -> Option<Self> {
        // SAFETY: both arguments may be null (default security, no name).
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        (!handle.is_null()).then_some(Self(handle))
    }

    fn assign(&self, child: &Child) -> bool {
        // SAFETY: `self.0` is a live job handle; the child's handle stays
        // valid while `child` is borrowed.
        unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) != 0 }
    }

    fn active_processes(&self) -> io::Result<u32> {
        // SAFETY: an all-zero accounting record is a valid initial value.
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        let size = u32::try_from(std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
            .map_err(io::Error::other)?;
        // SAFETY: the buffer is the documented structure for this class and
        // exactly `size` bytes long; the length out-pointer may be null.
        let ok = unsafe {
            QueryInformationJobObject(
                self.0,
                JobObjectBasicAccountingInformation,
                (&raw mut info).cast(),
                size,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info.ActiveProcesses)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle was created by CreateJobObjectW and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

pub struct Tracked {
    child: Child,
    job: Option<Job>,
}

// SAFETY: a job object handle may be used from any thread; `Tracked` is moved
// between threads but never used from two at once (`Running` takes `&mut`).
unsafe impl Send for Tracked {}

pub fn launch(launch: &Launch) -> io::Result<Tracked> {
    let mut command = Command::new(&launch.program);
    if !launch.args.is_empty() {
        command.raw_arg(&launch.args);
    }
    if let Some(dir) = launch.program.parent().filter(|dir| dir.is_dir()) {
        command.current_dir(dir);
    }
    let child = command.spawn()?;
    // Without a job (rare: a restricted parent job) the uninstaller is still
    // tracked by its own process, and the person can continue by hand.
    let job = Job::new().filter(|job| job.assign(&child));
    Ok(Tracked { child, job })
}

impl Running for Tracked {
    fn finished(&mut self) -> io::Result<bool> {
        let root_done = self.child.try_wait()?.is_some();
        match &self.job {
            Some(job) => Ok(job.active_processes()? == 0),
            None => Ok(root_done),
        }
    }
}

/// IDs of running processes whose executable is named `image` (case-insensitive).
pub fn running(image: &str) -> Vec<u32> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut found = Vec::new();
    // SAFETY: a process snapshot takes no pointers; the handle is closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return found;
    }
    // SAFETY: an all-zero entry is valid once dwSize is set.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0);
    // SAFETY: the snapshot is live and `entry` is sized for it.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        let length = entry
            .szExeFile
            .iter()
            .take_while(|&&unit| unit != 0)
            .count();
        let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if name.eq_ignore_ascii_case(image) {
            found.push(entry.th32ProcessID);
        }
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: the snapshot handle is owned here and closed once.
    unsafe { CloseHandle(snapshot) };
    found
}
