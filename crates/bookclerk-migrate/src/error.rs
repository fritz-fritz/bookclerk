use thiserror::Error;

/// Result type alias.
pub type Result<T> = std::result::Result<T, MigrateError>;

/// Migrate error.
#[derive(Debug, Error)]
pub enum MigrateError {
    /// Source variant.
    #[error("source classic Libation Files not found or incomplete: {0}")]
    Source(String),

    /// Settings variant.
    #[error("settings import error: {0}")]
    Settings(String),

    /// Accounts variant.
    #[error("accounts import error: {0}")]
    Accounts(String),

    /// Library variant.
    #[error("library database import error: {0}")]
    Library(String),

    /// Config variant.
    #[error("config error: {0}")]
    Config(#[from] bookclerk_config::ConfigError),

    /// Store variant.
    #[error("library store error: {0}")]
    Store(#[from] bookclerk_library::LibraryError),

    /// Io variant.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Sqlite variant.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Other variant.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
