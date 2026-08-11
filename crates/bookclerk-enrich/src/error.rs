use thiserror::Error;

/// Result type alias.
pub type Result<T> = std::result::Result<T, EnrichError>;

/// Enrich error.
#[derive(Debug, Error)]
pub enum EnrichError {
    /// Sync variant.
    #[error("enrichment error: {0}")]
    Sync(String),

    /// Io variant.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Library variant.
    #[error(transparent)]
    Library(#[from] bookclerk_library::LibraryError),
}
