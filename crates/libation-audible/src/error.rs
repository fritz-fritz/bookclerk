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

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
