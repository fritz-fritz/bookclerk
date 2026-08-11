use thiserror::Error;

/// Result type alias.
pub type Result<T> = std::result::Result<T, AcquireError>;

/// Acquire error.
#[derive(Debug, Error)]
pub enum AcquireError {
    /// Source variant.
    #[error("source error: {0}")]
    Source(#[from] bookclerk_source::SourceError),

    /// Storage variant.
    #[error("storage error: {0}")]
    Storage(#[from] bookclerk_storage::StorageError),

    /// Media variant.
    #[error("media error: {0}")]
    Media(#[from] bookclerk_media::MediaError),

    /// Library variant.
    #[error("library error: {0}")]
    Library(#[from] bookclerk_library::LibraryError),

    /// Io variant.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Other variant.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
