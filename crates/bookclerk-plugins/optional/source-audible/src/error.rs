use thiserror::Error;

/// Result type alias.
pub type Result<T> = std::result::Result<T, AudibleError>;

/// Audible error.
#[derive(Debug, Error)]
pub enum AudibleError {
    /// Auth variant.
    #[error("authentication error: {0}")]
    Auth(String),

    /// No accounts configured for this source (safe to skip in multi-source scan).
    #[error("no accounts configured: {0}")]
    NoAccounts(String),

    /// Account not found variant.
    #[error("account not found: {0}")]
    AccountNotFound(String),

    /// Import variant.
    #[error("import error: {0}")]
    Import(String),

    /// Sync variant.
    #[error("library sync error: {0}")]
    Sync(String),

    /// License variant.
    #[error("license error: {0}")]
    License(String),

    /// Adrm licenserequest returned 000307 — title is Widevine-only (no aaxc).
    #[error(
        "{asin}: no downloadable aaxc asset (Audible serves this title via Widevine only): {message}"
    )]
    NoAaxcAsset {
        /// Title ASIN that has no AAXC asset.
        asin: String,
        /// Upstream error detail from Audible.
        message: String,
    },

    /// Widevine variant.
    #[error("Widevine error: {0}")]
    Widevine(String),

    /// Download variant.
    #[error("download error: {0}")]
    Download(String),

    /// Io variant.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Other variant.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AudibleError {
    /// Is no AAXC asset.
    #[must_use]
    pub fn is_no_aaxc_asset(&self) -> bool {
        matches!(self, Self::NoAaxcAsset { .. })
    }
}

impl From<audible_rs::auth::login::LoginError> for AudibleError {
    fn from(err: audible_rs::auth::login::LoginError) -> Self {
        Self::Auth(err.to_string())
    }
}

impl From<audible_rs::auth::AuthError> for AudibleError {
    fn from(err: audible_rs::auth::AuthError) -> Self {
        Self::Auth(err.to_string())
    }
}

impl From<audible_rs::api::client::ApiError> for AudibleError {
    fn from(err: audible_rs::api::client::ApiError) -> Self {
        use audible_rs::api::client::ApiError;
        match err {
            ApiError::LicenseRejected {
                asin,
                error_code,
                message,
                ..
            } if error_code == "000307" => Self::NoAaxcAsset { asin, message },
            ApiError::LicenseRejected {
                asin,
                error_code,
                message,
                request_id,
                ..
            } => Self::License(format!(
                "{asin}: license rejected ({error_code}): {message} (request_id={request_id})"
            )),
            ApiError::LicenseDenied(message) => Self::License(format!("license denied: {message}")),
            other => Self::Sync(other.to_string()),
        }
    }
}

impl From<bookclerk_library::LibraryError> for AudibleError {
    fn from(err: bookclerk_library::LibraryError) -> Self {
        Self::Other(err.into())
    }
}
