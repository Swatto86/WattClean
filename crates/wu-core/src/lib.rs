//! The uninstall engine, independent of Windows APIs.
//!
//! Everything that touches the machine's registry, processes or privileges
//! goes through [`System`]; the real implementation lives in `wu-windows`, a
//! fake one (feature `fake`) drives the tests and the WebDriver suite. Files
//! are handled here with `std::fs`, on paths the registry names. Those
//! tests use Windows paths and run on Windows only
//! (`cfg(test)` with `cfg(windows)`).
//!
//! On Linux and macOS there is no uninstall registry: [`packages`] asks the
//! package managers instead.

pub mod backup;
pub mod command_line;
mod error;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod folders;
pub mod history;
pub mod identity;
pub mod install_log;
pub mod model;
pub mod msi;
pub mod packages;
pub mod paths;
pub mod programs;
pub mod scan;
pub mod settings;
pub mod store;
pub mod system;
pub mod trace;

pub use error::{Error, Result};
pub use model::{Hive, Program, RawValue, RegPath, Scope};
pub use system::{Launch, Running, System, Tracing};
