use thiserror::Error;

pub type Result<T> = std::result::Result<T, AudibleError>;

#[derive(Debug, Error)]
pub enum AudibleError {
    #[error("authentication error: {0}")]
    Auth(String),

    #[error("account not found: {0}")]
    AccountNotFound(String),

    #[error("import error: {0}")]
    Import(String),

    #[error("library sync error: {0}")]
    Sync(String),

    #[error("license error: {0}")]
    License(String),

    #[error("download error: {0}")]
    Download(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
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
                request_id,
                ..
            } if error_code == "000307" => Self::License(format!(
                "{asin}: no downloadable aaxc asset (Audible serves this title via Widevine only). \
                 Widevine/CENC liberate is not implemented yet. server={message} request_id={request_id}"
            )),
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

impl From<libation_library::LibraryError> for AudibleError {
    fn from(err: libation_library::LibraryError) -> Self {
        Self::Other(err.into())
    }
}

