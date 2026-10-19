//! The real Linux or macOS machine for the uninstaller's package engine
//! (`wu_core::packages`). On Windows this crate is empty: the uninstaller
//! reads the registry there (`wu-windows`).

#[cfg(unix)]
mod machine;
#[cfg(unix)]
mod trash;

#[cfg(unix)]
pub use machine::UnixPackages;
