//! Integration errors.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, IntegrationError>;

/// Errors from outbound integrations and the connect portal.
#[derive(Debug, Error)]
pub enum IntegrationError {
    /// Remote HTTP API returned a non-success status.
    #[error("integration API error ({status}): {message}")]
    Api {
        /// HTTP status code from the remote API.
        status: u16,
        /// Response body or summarized error text.
        message: String,
    },

    /// Library variant.
    #[error(transparent)]
    Library(#[from] bookclerk_library::LibraryError),

    /// Config variant.
    #[error(transparent)]
    Config(#[from] bookclerk_config::ConfigError),

    /// Other variant.
    #[error("{0}")]
    Other(#[from] anyhow::Error),

    /// Message variant.
    #[error("{0}")]
    Message(String),
}

impl IntegrationError {
    /// Message.
    #[must_use]
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }

    /// API.
    #[must_use]
    pub fn api(status: u16, message: impl Into<String>) -> Self {
        Self::Api {
            status,
            message: message.into(),
        }
    }
}
