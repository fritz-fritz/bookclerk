use thiserror::Error;

/// Result type alias.
pub type Result<T> = std::result::Result<T, StorageError>;

/// Storage error.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Not found variant.
    #[error("object not found: {0}")]
    NotFound(String),

    /// Invalid key variant.
    #[error("invalid storage key: {0}")]
    InvalidKey(String),

    /// Io variant.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// S3 variant.
    #[error("S3 error: {0}")]
    S3(String),

    /// Other variant.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
