//! Catalog and install errors.

use thiserror::Error;

/// Errors from discovery, manifest validation, or installation.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// Message variant.
    #[error("{0}")]
    Message(String),
    /// Io variant.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON variant.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// TOML variant.
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
    /// URL variant.
    #[error(transparent)]
    Url(#[from] url::ParseError),
}

impl CatalogError {
    /// Message.
    #[must_use]
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, CatalogError>;
