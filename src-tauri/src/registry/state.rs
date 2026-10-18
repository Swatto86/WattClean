//! What the app holds while it runs: the machine it acts on, where its own
//! files live, the last scan and search, the watcher, and an open menu.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use wrc_core::System;
use wrc_core::scan::{Progress, Report};
use wrc_core::watch::Watcher;

use crate::error::CommandError;

pub struct AppState {
    pub system: Arc<dyn System>,
    pub data_dir: PathBuf,
    pub watcher: Watcher,
    /// The scan or search running now, so the page can follow or stop it.
    progress: Mutex<Option<Arc<Progress>>>,
    /// The last results, which a clean refers to by number: the page never
    /// sends a registry path to delete.
    cleaner: Mutex<Option<Report>>,
    search: Mutex<Option<Report>>,
    /// The context menu open now, and where to send the item chosen.
    menu: Mutex<Option<(u64, Sender<String>)>>,
    next_menu: AtomicU64,
}

/// Which results a command means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum List {
    Cleaner,
    Search,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Each field is replaced whole, so a panic elsewhere cannot leave one
    // half-written.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl AppState {
    pub fn new(system: Arc<dyn System>, data_dir: PathBuf, watcher: Watcher) -> Self {
        Self {
            system,
            data_dir,
            watcher,
            progress: Mutex::new(None),
            cleaner: Mutex::new(None),
            search: Mutex::new(None),
            menu: Mutex::new(None),
            next_menu: AtomicU64::new(1),
        }
    }

    pub fn backups_dir(&self) -> PathBuf {
        wrc_core::settings::backups_dir(&self.data_dir)
    }

    /// Start following a scan; refused while another one runs.
    pub fn begin_scan(&self) -> Result<Arc<Progress>, CommandError> {
        let mut running = lock(&self.progress);
        if running.is_some() {
            return Err("A scan is already running.".into());
        }
        let progress = Arc::new(Progress::default());
        *running = Some(Arc::clone(&progress));
        Ok(progress)
    }

    pub fn end_scan(&self) {
        *lock(&self.progress) = None;
    }

    pub fn progress(&self) -> Option<Arc<Progress>> {
        lock(&self.progress).clone()
    }

    fn results(&self, list: List) -> &Mutex<Option<Report>> {
        match list {
            List::Cleaner => &self.cleaner,
            List::Search => &self.search,
        }
    }

    pub fn set_report(&self, list: List, report: Report) {
        *lock(self.results(list)) = Some(report);
    }

    pub fn report(&self, list: List) -> Result<Report, CommandError> {
        lock(self.results(list))
            .clone()
            .ok_or_else(|| CommandError::from("Those results are gone; scan again."))
    }

    /// Forget findings that were removed, so they are never removed twice.
    pub fn drop_findings(&self, list: List, removed: &[u32]) {
        if let Some(report) = lock(self.results(list)).as_mut() {
            report
                .findings
                .retain(|finding| !removed.contains(&finding.id));
        }
    }

    /// Note a menu about to open; its chosen item arrives on `sender`.
    pub fn open_menu(&self, sender: Sender<String>) -> u64 {
        let id = self.next_menu.fetch_add(1, Ordering::Relaxed);
        *lock(&self.menu) = Some((id, sender));
        id
    }

    /// Deliver a chosen item to the menu it belongs to.
    pub fn menu_chosen(&self, menu: u64, item: &str) {
        if let Some((open, sender)) = lock(&self.menu).as_ref()
            && *open == menu
        {
            let _ = sender.send(item.to_owned());
        }
    }

    pub fn close_menu(&self, menu: u64) {
        let mut open = lock(&self.menu);
        if open.as_ref().is_some_and(|(id, _)| *id == menu) {
            *open = None;
        }
    }
}
