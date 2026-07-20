use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, DecryptError>;

#[derive(Debug, Error)]
pub enum DecryptError {
    #[error("input file missing: {0}")]
    InputMissing(PathBuf),

    #[error("decrypt output missing: {0}")]
    OutputMissing(PathBuf),

    #[error("aaxclean-cli not found at {0}; install it or set decrypt.aaxclean_bin")]
    AaxcleanNotFound(PathBuf),

    #[error("aaxclean-cli failed (status={status:?}): {stderr}")]
    AaxcleanFailed { status: Option<i32>, stderr: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
