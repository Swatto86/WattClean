//! What the app holds while it runs: the machine it acts on, where its own
//! files live, and the uninstalls in progress.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use wu_core::{System, Tracing};

use crate::error::CommandError;
use crate::uninstall::session::Session;

pub struct AppState {
    pub system: Arc<dyn System>,
    pub data_dir: PathBuf,
    /// Folders a scan never offers: the app's data and its own program.
    pub protected: Vec<PathBuf>,
    sessions: Mutex<HashMap<u32, Arc<Session>>>,
    traces: Mutex<HashMap<u32, TraceRun>>,
    next_id: AtomicU32,
}

/// An installer running under an install trace.
pub struct TraceRun {
    pub tracing: Box<dyn Tracing>,
    /// The installer's file name, as the log will record it.
    pub installer: String,
}

impl AppState {
    /// `root` is WattClean's data folder, holding every module's files.
    pub fn new(system: Arc<dyn System>, data_dir: PathBuf, root: &Path, fake: bool) -> Self {
        // The test machine lives inside the data folder, so that build
        // protects only the app's own records in it.
        let mut protected = if fake {
            vec![data_dir.join("backups"), data_dir.join("logs")]
        } else {
            vec![root.to_path_buf()]
        };
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
        {
            protected.push(dir);
        }
        Self {
            system,
            data_dir,
            protected,
            sessions: Mutex::new(HashMap::new()),
            traces: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
        }
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    fn sessions(&self) -> MutexGuard<'_, HashMap<u32, Arc<Session>>> {
        // Sessions are only inserted and removed whole, so a panic elsewhere
        // cannot leave the map half-changed.
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn add_session(&self, build: impl FnOnce(u32) -> Session) -> Arc<Session> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let session = Arc::new(build(id));
        self.sessions().insert(id, Arc::clone(&session));
        session
    }

    pub fn session(&self, id: u32) -> Result<Arc<Session>, CommandError> {
        self.sessions()
            .get(&id)
            .cloned()
            .ok_or_else(|| CommandError::from("That uninstall is no longer open."))
    }

    pub fn end_session(&self, id: u32) -> Option<Arc<Session>> {
        self.sessions().remove(&id)
    }

    fn traces(&self) -> MutexGuard<'_, HashMap<u32, TraceRun>> {
        // Runs are only inserted and removed whole, like sessions.
        self.traces
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn add_trace(&self, run: TraceRun) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.traces().insert(id, run);
        id
    }

    pub fn trace_finished(&self, id: u32) -> Result<bool, CommandError> {
        let mut traces = self.traces();
        let run = traces
            .get_mut(&id)
            .ok_or_else(|| CommandError::from("That install is no longer being traced."))?;
        run.tracing
            .finished()
            .map_err(|error| CommandError::from(format!("Could not follow the installer: {error}")))
    }

    pub fn take_trace(&self, id: u32) -> Result<TraceRun, CommandError> {
        self.traces()
            .remove(&id)
            .ok_or_else(|| CommandError::from("That install is no longer being traced."))
    }
}
