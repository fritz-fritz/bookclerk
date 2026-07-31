//! Storage backends for acquired audiobooks, covers, and PDFs.

mod error;
mod fanout;
mod local;
mod s3;
mod s3_credentials;
mod traits;

pub use error::{Result, StorageError};
pub use fanout::FanoutBackend;
pub use local::{LocalFsBackend, LocalFsOwner};
pub use s3::S3Backend;
pub use s3_credentials::{
    delete_s3_credentials, load_s3_credentials, save_s3_credentials, S3Credentials,
    ENV_AWS_ACCESS_KEY_ID, ENV_AWS_SECRET_ACCESS_KEY, ENV_AWS_SESSION_TOKEN, S3_SECRET_NAME,
};
pub use traits::{
    bookclerk_meta_sidecar_key, is_audio_key, ObjectInfo, ObjectMeta, ObjectProbe, StorageBackend,
    AUDIO_EXTENSIONS,
};

use bookclerk_config::{normalize_storage_prefix, Config, OutputBackendKind};
use sea_orm::DatabaseConnection;

/// Build the configured storage backend(s).
///
/// When multiple `[output.*]` destination plugins are enabled, returns a
/// [`FanoutBackend`] that writes to all of them.
///
/// Pass `db` so the S3 destination can load credentials from `encrypted_secrets`
/// after env override (`BOOKCLERK_AWS_*`) and before the AWS SDK default chain.
pub async fn from_config(
    config: &Config,
    db: Option<&DatabaseConnection>,
) -> Result<Box<dyn StorageBackend>> {
    config
        .output
        .validate_destinations()
        .map_err(|err| StorageError::InvalidKey(err.to_string()))?;

    let mut backends: Vec<Box<dyn StorageBackend>> = Vec::new();
    for kind in config.output.enabled_backends() {
        match kind {
            OutputBackendKind::Local => {
                backends.push(Box::new(local_fs_from_config(config)?));
            }
            OutputBackendKind::S3 => {
                let prefix = normalize_storage_prefix(config.output.s3.prefix.trim());
                backends.push(Box::new(
                    S3Backend::from_config(&config.output.s3, &prefix, db).await?,
                ));
            }
        }
    }

    if backends.len() == 1 {
        return Ok(backends.remove(0));
    }
    Ok(Box::new(FanoutBackend::new(backends)?))
}

pub(crate) fn normalize_prefix(prefix: &str) -> String {
    normalize_storage_prefix(prefix)
}

/// Build a [`LocalFsBackend`] from `[output.local]`, applying owner identity when set.
pub fn local_fs_from_config(config: &Config) -> Result<LocalFsBackend> {
    let prefix = normalize_storage_prefix(config.output.local.prefix.trim());
    let owner = local_owner_from_config(config);
    if let Some(ref owner) = owner {
        log_local_owner(config, owner);
    }
    LocalFsBackend::with_prefix_and_owner(config.output.local.root.clone(), &prefix, owner)
}

#[cfg(unix)]
fn local_owner_from_config(config: &Config) -> Option<LocalFsOwner> {
    bookclerk_config::resolve_local_file_owner(&config.output.local).map(|o| LocalFsOwner {
        uid: o.uid,
        gid: o.gid,
    })
}

#[cfg(windows)]
fn local_owner_from_config(config: &Config) -> Option<LocalFsOwner> {
    bookclerk_config::resolve_local_file_owner(&config.output.local).map(|o| LocalFsOwner {
        user: o.user,
        group: o.group,
    })
}

#[cfg(not(any(unix, windows)))]
fn local_owner_from_config(_config: &Config) -> Option<LocalFsOwner> {
    None
}

#[cfg(unix)]
fn log_local_owner(config: &Config, owner: &LocalFsOwner) {
    tracing::debug!(
        root = %config.output.local.root.display(),
        uid = owner.uid,
        gid = owner.gid,
        "local output will chown acquired files to configured owner"
    );
}

#[cfg(windows)]
fn log_local_owner(config: &Config, owner: &LocalFsOwner) {
    tracing::debug!(
        root = %config.output.local.root.display(),
        user = %owner.user,
        group = ?owner.group,
        "local output will set ownership on acquired files"
    );
}

#[cfg(not(any(unix, windows)))]
fn log_local_owner(_config: &Config, _owner: &LocalFsOwner) {}
