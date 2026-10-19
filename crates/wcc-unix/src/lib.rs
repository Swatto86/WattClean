//! The real Linux or macOS machine for WattClean's engine. On Windows this
//! crate is empty (`wcc-windows` is the machine there).
//!
//! `process` (finding a program, running it with administrator rights) is
//! also used by the uninstaller's `wu-unix`.

#[cfg(unix)]
mod ctrl_c;
#[cfg(unix)]
mod drives;
#[cfg(unix)]
mod folders;
#[cfg(unix)]
mod machine;
#[cfg(unix)]
pub mod process;
#[cfg(unix)]
mod sys;
#[cfg(unix)]
mod trash;

#[cfg(unix)]
pub use ctrl_c::stop_on_ctrl_c;
#[cfg(unix)]
pub use folders::Folders;
#[cfg(unix)]
pub use machine::UnixSystem;
#[cfg(unix)]
pub use sys::is_elevated;
