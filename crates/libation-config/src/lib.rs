//! Configuration, data-directory resolution, and logging setup for Libation.

mod error;
mod logging;
mod paths;
mod settings;

pub use error::{ConfigError, Result};
pub use logging::{init_tracing, LogFormat};
pub use paths::{resolve_files_dir, Paths};
pub use settings::{
    AudioQuality, Config, DaemonConfig, DownloadConfig, DownloadFormat, LibraryConfig,
    StorageBackendKind, StorageConfig, StorageLocalConfig, StorageS3Config,
};
