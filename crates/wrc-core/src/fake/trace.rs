//! The fake machine's event tracing: every change made through the port is
//! a write by this process, as Windows would report the app's own writes.
//! Changes made with `FakeSystem::put` are not seen, like writes through a
//! handle opened before tracing started.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use super::State;
use crate::model::{RawValue, RegPath};
use crate::trace::{Actor, Captured, RegistryTrace, Write, WriteOp};

/// The fake keeps this many writes, newest last.
const KEPT: usize = 1_000;

impl State {
    pub(super) fn traced(&mut self, op: WriteOp, key: &RegPath, value: Option<&str>) {
        self.traced_data(op, key, value, None, None);
    }

    /// A value set, with the data written and what it replaced, as
    /// Windows' event carries them.
    pub(super) fn traced_set(&mut self, key: &RegPath, new: &RawValue, old: Option<&RawValue>) {
        self.traced_data(
            WriteOp::ValueSet,
            key,
            Some(&new.name),
            Some(captured(new)),
            old.map(captured),
        );
    }

    fn traced_data(
        &mut self,
        op: WriteOp,
        key: &RegPath,
        value: Option<&str>,
        new: Option<Captured>,
        old: Option<Captured>,
    ) {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        self.writes.push_back(Write {
            time,
            actor: this_process(),
            op,
            key: key.clone(),
            value: value.map(str::to_owned),
            new,
            old,
        });
        while self.writes.len() > KEPT {
            self.writes.pop_front();
        }
    }
}

fn captured(value: &RawValue) -> Captured {
    Captured {
        kind: value.kind,
        size: u32::try_from(value.bytes.len()).unwrap_or(u32::MAX),
        bytes: value.bytes.clone(),
    }
}

fn this_process() -> Actor {
    let process = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    Actor {
        pid: std::process::id(),
        process,
    }
}

pub struct FakeTrace {
    state: Arc<Mutex<State>>,
    followed: Mutex<Vec<RegPath>>,
}

impl FakeTrace {
    /// Like Windows, tracing needs administrator rights.
    pub(super) fn start(state: &Arc<Mutex<State>>) -> io::Result<Self> {
        if !state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .elevated
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "event tracing needs administrator rights",
            ));
        }
        Ok(Self {
            state: Arc::clone(state),
            followed: Mutex::new(Vec::new()),
        })
    }
}

impl RegistryTrace for FakeTrace {
    fn follow(&self, keys: &[RegPath]) {
        *self.followed.lock().unwrap_or_else(PoisonError::into_inner) = keys.to_vec();
    }

    fn writes(&self, from: u64, to: u64) -> Vec<Write> {
        let followed = self
            .followed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .writes
            .iter()
            .filter(|write| (from..=to).contains(&write.time))
            .filter(|write| followed.iter().any(|key| write.key.is_within(key)))
            .cloned()
            .collect()
    }
}
