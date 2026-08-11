//! Manifest parse / validate errors.

use thiserror::Error;

/// Result alias for manifest operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors from TOML parse or semantic validation.
#[derive(Debug, Error)]
pub enum Error {
    /// Message variant.
    #[error("{0}")]
    Message(String),
    /// TOML de variant.
    #[error("toml: {0}")]
    TomlDe(#[from] toml::de::Error),
    /// TOML ser variant.
    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

impl Error {
    /// Message.
    #[must_use]
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}
