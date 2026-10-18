//! WattClean's engine. It knows nothing of Tauri or Win32: the machine is
//! reached through [`system::System`], implemented by `wcc-windows` for the
//! real PC and by [`fake`] for tests.

pub mod catalog;
pub mod cleanerml;
pub mod defs;
pub mod edit;
pub mod exec;
pub mod history;
pub mod model;
pub mod ops;
pub mod paths;
pub mod report;
pub mod run;
pub mod search;
pub mod settings;
pub mod shred;
pub mod sqlite;
pub mod store;
pub mod system;
pub mod vars;
pub mod wildcard;
pub mod winapp2;

#[cfg(any(test, feature = "fake"))]
pub mod fake;
#[cfg(any(test, feature = "fake"))]
mod fake_seed;
