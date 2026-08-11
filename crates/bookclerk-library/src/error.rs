use thiserror::Error;

/// Result type alias.
pub type Result<T> = std::result::Result<T, LibraryError>;

/// Library error.
#[derive(Debug, Error)]
pub enum LibraryError {
    /// Database variant.
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    /// Migrate variant.
    #[error("migration error: {0}")]
    Migrate(#[from] rusqlite_migration::Error),

    /// Orm variant.
    #[error("ORM / database plugin error: {0}")]
    Orm(#[from] sea_orm::DbErr),

    /// Config variant.
    #[error("config error: {0}")]
    Config(#[from] bookclerk_config::ConfigError),

    /// Io variant.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Not found variant.
    #[error("book not found: {0}")]
    NotFound(String),

    /// Other variant.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
