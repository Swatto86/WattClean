//! The one error shape the page receives: a sentence it can show.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub message: String,
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl From<wrc_core::Error> for CommandError {
    fn from(error: wrc_core::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl From<wu_core::Error> for CommandError {
    fn from(error: wu_core::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl From<tauri::Error> for CommandError {
    fn from(error: tauri::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

/// Runs blocking engine work off the async runtime.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| CommandError::from(format!("background work failed: {error}")))?
}
