//! What the app holds while it runs: the machine it acts on, its data
//! folder, the settings, the cleaner catalog, and the one job that may run
//! at a time (a preview, clean, shred or wipe) with its stop flag.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use wcc_core::catalog::Catalog;
use wcc_core::run::RunSettings;
use wcc_core::settings::Settings;
use wcc_core::store;
use wcc_core::system::System;

use crate::error::{CommandError, CommandResult};

pub struct AppState {
    pub system: Arc<dyn System>,
    pub data_dir: PathBuf,
    /// True in the WebDriver suite's build, which acts on a test machine.
    pub fake: bool,
    settings: Mutex<Settings>,
    catalog: RwLock<Arc<Catalog>>,
    busy: AtomicBool,
    cancel: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // The settings are replaced whole, so a panic elsewhere cannot leave
    // them half-written.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Marks the app busy for one job; dropping it frees the app again.
pub struct Job<'a> {
    state: &'a AppState,
}

impl Drop for Job<'_> {
    fn drop(&mut self) {
        self.state.busy.store(false, Ordering::SeqCst);
    }
}

impl AppState {
    pub fn new(system: Arc<dyn System>, data_dir: PathBuf, fake: bool) -> Self {
        let settings = Settings::load(&data_dir.join("settings.json"));
        let catalog = Catalog::load(Some(&store::cleaners_dir(&data_dir)), settings.community);
        Self {
            system,
            data_dir,
            fake,
            settings: Mutex::new(settings),
            catalog: RwLock::new(Arc::new(catalog)),
            busy: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    /// Changes the settings under one lock, so two changes at once cannot
    /// undo each other; saves first, then keeps, so a failed save leaves
    /// the old settings. Returns them as they were and as they are now.
    pub fn update_settings(
        &self,
        change: impl FnOnce(&mut Settings) -> CommandResult<()>,
    ) -> CommandResult<(Settings, Settings)> {
        let mut current = lock(&self.settings);
        let mut next = current.clone();
        change(&mut next)?;
        next.save(&self.settings_path()).map_err(|error| {
            CommandError::from(format!("The settings could not be saved: {error}"))
        })?;
        let before = std::mem::replace(&mut *current, next.clone());
        Ok((before, next))
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    pub fn history_path(&self) -> PathBuf {
        self.data_dir.join("history.json")
    }

    pub fn cleaners_dir(&self) -> PathBuf {
        store::cleaners_dir(&self.data_dir)
    }

    pub fn catalog(&self) -> Arc<Catalog> {
        Arc::clone(&self.catalog.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Reads the cleaner files again (after an import or a change of the
    /// community setting).
    pub fn reload_catalog(&self) {
        let community = self.settings().community;
        let catalog = Catalog::load(Some(&self.cleaners_dir()), community);
        *self.catalog.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(catalog);
    }

    /// The settings a preview or clean runs with.
    pub fn run_settings(&self) -> RunSettings {
        let settings = self.settings();
        RunSettings {
            keep: settings
                .keep
                .iter()
                .map(|k| PathBuf::from(&k.path))
                .collect(),
            custom_paths: settings.custom_paths,
            shred: settings.shred,
            delete_on_reboot: settings.delete_on_reboot,
        }
    }

    /// Starts a job, or says one is already running.
    pub fn start_job(&self) -> CommandResult<Job<'_>> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("Another clean, shred or wipe is still running.".into());
        }
        self.cancel.store(false, Ordering::SeqCst);
        Ok(Job { state: self })
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}
