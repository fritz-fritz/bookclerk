//! Error types for content sources.

use thiserror::Error;

/// Result alias for content-source operations.
pub type Result<T> = std::result::Result<T, SourceError>;

/// Errors from content-source auth, scan, or fetch.
#[derive(Debug, Error)]
pub enum SourceError {
    /// No accounts are configured for this source (safe to skip in multi-source scan).
    #[error("no accounts configured for source: {0}")]
    NoAccounts(String),
    /// Auth variant.
    #[error("{0}")]
    Auth(String),
    /// API variant.
    #[error("{0}")]
    Api(String),
    /// Io variant.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Library variant.
    #[error("{0}")]
    Library(#[from] bookclerk_library::LibraryError),
    /// Other variant.
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl SourceError {
    /// No accounts.
    #[must_use]
    pub fn no_accounts(msg: impl Into<String>) -> Self {
        Self::NoAccounts(msg.into())
    }

    /// Auth.
    #[must_use]
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    /// API.
    #[must_use]
    pub fn api(msg: impl Into<String>) -> Self {
        Self::Api(msg.into())
    }
}
