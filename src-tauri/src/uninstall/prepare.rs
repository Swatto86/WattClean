//! What a new uninstall session starts from, besides the program's entry:
//! the folder a forced uninstall names, and the program's install log.

use std::path::{Path, PathBuf};

use wu_core::Program;
use wu_core::folders::Folders;
use wu_core::identity::Identity;
use wu_core::install_log::InstallLog;

use crate::error::CommandResult;

/// Validate a forced uninstall's folder: absolute, existing, a folder.
pub fn forced_folder(text: &str, folders: &Folders) -> CommandResult<Option<PathBuf>> {
    let text = text.trim().trim_matches('"');
    if text.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(text);
    if !path.is_absolute() || !path.is_dir() {
        return Err(
            "Choose an existing folder, written as a full path such as C:\\Program Files\\App."
                .into(),
        );
    }
    // Everything registered under the folder would be ticked for removal, so
    // Windows' folders, the drive root and the shared roots are refused.
    if !Identity::is_own_folder(&path, folders) {
        return Err(
            "That folder is shared by Windows or other programs. Choose the program's own folder, such as C:\\Program Files\\App, or leave it empty."
                .into(),
        );
    }
    Ok(Some(path))
}

/// The program's install log, if it was installed with tracing. A program
/// whose entry names no folder gets the folders the log saw it create.
pub fn attach_log(
    data_dir: &Path,
    folders: &Folders,
    program: &Program,
    identity: &mut Identity,
) -> Option<InstallLog> {
    let log = wu_core::install_log::for_program(data_dir, program)?;
    if identity.install_dirs.is_empty() {
        identity.install_dirs = log.install_dirs(folders);
        identity.read_executables();
    }
    Some(log)
}
