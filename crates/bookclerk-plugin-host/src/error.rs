//! Plugin host/guest errors.

use thiserror::Error;

/// Result alias for plugin operations.
pub type Result<T> = std::result::Result<T, PluginError>;

/// Errors from discovery, RPC, or plugin adapters.
#[derive(Debug, Error)]
pub enum PluginError {
    /// Message variant.
    #[error("{0}")]
    Message(String),
    /// Io variant.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON variant.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// TOML variant.
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),
    /// Manifest variant.
    #[error(transparent)]
    Manifest(#[from] bookclerk_plugin_manifest::Error),
    /// Other variant.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl PluginError {
    /// Message.
    #[must_use]
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

impl From<PluginError> for bookclerk_source::SourceError {
    fn from(err: PluginError) -> Self {
        bookclerk_source::SourceError::api(err.to_string())
    }
}

impl From<PluginError> for bookclerk_integrations::IntegrationError {
    fn from(err: PluginError) -> Self {
        bookclerk_integrations::IntegrationError::message(err.to_string())
    }
}
