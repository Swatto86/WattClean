//! The fake machine's change notification: any change to its registry
//! counts as a change beneath every watched key.

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::State;
use crate::system::KeyWatch;

/// Reports a change whenever anything in the fake registry changed.
pub(super) struct FakeWatch {
    pub(super) state: Arc<Mutex<State>>,
    pub(super) seen: u64,
}

impl KeyWatch for FakeWatch {
    fn wait(&mut self, timeout: Duration) -> io::Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            let now = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .generation;
            if now != self.seen {
                self.seen = now;
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
