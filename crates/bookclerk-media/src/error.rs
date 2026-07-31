use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, MediaError>;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("input file missing: {0}")]
    InputMissing(PathBuf),

    #[error("output file missing: {0}")]
    OutputMissing(PathBuf),

    #[error("MP4 parse/remux error: {0}")]
    Mp4(String),

    #[error("media processing failed: {0}")]
    Native(String),

    /// A media worker process could not be started, died, or returned a reply
    /// that could not be parsed. Distinct from [`MediaError::Native`] so a
    /// crashed codec is not mistaken for a malformed file.
    #[error("media worker ({job}) failed: {detail}")]
    Worker {
        /// Which operation was running.
        job: &'static str,
        /// What went wrong.
        detail: String,
    },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
