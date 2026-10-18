//! The registry cleaning engine, independent of Windows APIs.
//!
//! Everything that touches the machine's registry, drives or shell goes
//! through [`System`]; the real implementation lives in `wrc-windows`, a
//! fake one (feature `fake`) drives the tests and the WebDriver suite. Files
//! a registry entry names are checked here with `std::fs`.

pub mod autostart;
pub mod backup;
pub mod check;
mod error;
pub mod export;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod format;
pub mod jobs;
pub mod keyname;
pub mod launch;
pub mod model;
pub mod paths;
pub mod scan;
pub mod search;
pub mod settings;
pub mod store;
pub mod system;
pub mod trace;
pub mod tuner;
pub mod watch;
mod xlsx;

pub use error::{Error, Result};
pub use model::{Hive, RawValue, RegPath};
pub use system::System;
