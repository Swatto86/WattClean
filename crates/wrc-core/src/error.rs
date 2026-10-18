use std::io;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("registry {op} failed for {key}: {source}")]
    Registry {
        op: &'static str,
        key: String,
        source: io::Error,
    },
    #[error("{op} failed for {}: {source}", path.display())]
    File {
        op: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error("{} is unreadable ({reason}); it was left untouched", path.display())]
    Unreadable { path: PathBuf, reason: String },
    #[error("{0}")]
    Invalid(String),
}

impl Error {
    pub fn file(op: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::File {
            op,
            path: path.into(),
            source,
        }
    }

    pub fn registry(op: &'static str, key: &crate::RegPath, source: io::Error) -> Self {
        Self::Registry {
            op,
            key: key.display(),
            source,
        }
    }
}
