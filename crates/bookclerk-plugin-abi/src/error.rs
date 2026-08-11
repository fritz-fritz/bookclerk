//! Plugin error model shared by host and guests.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result alias for ABI operations.
pub type Result<T> = std::result::Result<T, PluginError>;

/// Stable error codes (schema `PluginError.code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginErrorCode {
    /// Invalid params variant.
    InvalidParams,
    /// Unauthorized variant.
    Unauthorized,
    /// Forbidden variant.
    Forbidden,
    /// Not found variant.
    NotFound,
    /// Unavailable variant.
    Unavailable,
    /// Unsupported variant.
    Unsupported,
    /// Internal variant.
    Internal,
}

impl PluginErrorCode {
    /// As str.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidParams => "invalid_params",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::Internal => "internal",
        }
    }
}

impl std::fmt::Display for PluginErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// RPC/plugin failure payload.
#[derive(Debug, Clone, Error, Serialize, Deserialize, PartialEq, Eq)]
#[error("{code}: {message}")]
pub struct PluginError {
    /// Code.
    pub code: PluginErrorCode,
    /// Message.
    pub message: String,
    /// Details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Map<String, serde_json::Value>>,
}

impl PluginError {
    /// New.
    #[must_use]
    pub fn new(code: PluginErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    /// Invalid params.
    #[must_use]
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(PluginErrorCode::InvalidParams, message)
    }

    /// Unsupported.
    #[must_use]
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(PluginErrorCode::Unsupported, message)
    }

    /// Internal.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(PluginErrorCode::Internal, message)
    }

    /// Not found.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(PluginErrorCode::NotFound, message)
    }
}
