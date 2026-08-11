use thiserror::Error;

/// Result alias for config operations.
pub type Result<T> = std::result::Result<T, ConfigError>;

/// Errors produced while loading or validating configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Read variant.
    #[error("failed to read config from {path}: {source}")]
    Read {
        /// Path.
        path: String,
        /// Source.
        #[source]
        source: std::io::Error,
    },

    /// Parse variant.
    #[error("failed to parse config from {path}: {source}")]
    Parse {
        /// Path.
        path: String,
        /// Source.
        #[source]
        source: toml::de::Error,
    },

    /// Invalid variant.
    #[error("invalid configuration: {0}")]
    Invalid(String),

    /// Io variant.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
