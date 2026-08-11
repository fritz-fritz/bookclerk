//! Destination-specific storage backends for one acquire operation.

use bookclerk_config::{normalize_storage_prefix, Config, MultiDestinationMode, OutputBackendKind};
use bookclerk_library::LibraryStore;
use bookclerk_source::DownloadOptions;
use bookclerk_storage::{FanoutBackend, LocalFsBackend, S3Backend, StorageBackend};

use crate::error::{AcquireError, Result};

/// Acquire destination.
pub struct AcquireDestination {
    /// Kind.
    pub kind: OutputBackendKind,
    /// Backend.
    pub backend: Box<dyn StorageBackend>,
    /// Options.
    pub options: DownloadOptions,
}

/// Acquire destinations.
pub struct AcquireDestinations {
    /// Items.
    pub items: Vec<AcquireDestination>,
    /// Primary.
    pub primary: OutputBackendKind,
    /// Multi destination.
    pub multi_destination: MultiDestinationMode,
}

impl AcquireDestinations {
    /// Build destination backends.
    ///
    /// `library` is used when the S3 destination loads credentials from
    /// `encrypted_secrets` (after `BOOKCLERK_AWS_*` env override, before
    /// the AWS SDK default chain).
    pub async fn from_config(config: &Config, library: Option<&LibraryStore>) -> Result<Self> {
        config.output.validate_destinations().map_err(|err| {
            AcquireError::Other(anyhow::anyhow!("invalid output destination config: {err}"))
        })?;

        let primary = config.output.primary_backend().ok_or_else(|| {
            AcquireError::Other(anyhow::anyhow!(
                "enable at least one of [output.local] or [output.s3]"
            ))
        })?;
        let db = library.map(|store| store.db());
        let mut items = Vec::new();
        for kind in config.output.enabled_backends() {
            let backend: Box<dyn StorageBackend> = match kind {
                OutputBackendKind::Local => {
                    let prefix = normalize_storage_prefix(config.output.local.prefix.trim());
                    Box::new(LocalFsBackend::with_prefix(
                        config.output.local.root.clone(),
                        &prefix,
                    )?)
                }
                OutputBackendKind::S3 => {
                    let prefix = normalize_storage_prefix(config.output.s3.prefix.trim());
                    Box::new(S3Backend::from_config(&config.output.s3, &prefix, db).await?)
                }
            };
            items.push(AcquireDestination {
                kind,
                backend,
                options: DownloadOptions::for_output_backend(config, kind),
            });
        }
        Ok(Self {
            items,
            primary,
            multi_destination: config.output.multi_destination,
        })
    }

    /// Len.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Primary destination.
    #[must_use]
    pub fn primary_destination(&self) -> &AcquireDestination {
        self.items
            .iter()
            .find(|dest| dest.kind == self.primary)
            .unwrap_or_else(|| &self.items[0])
    }

    /// Destination.
    #[must_use]
    pub fn destination(&self, kind: OutputBackendKind) -> Option<&AcquireDestination> {
        self.items.iter().find(|dest| dest.kind == kind)
    }

    /// Clone destination backends into a listing/match storage handle.
    ///
    /// Prefer this over a second [`bookclerk_storage::from_config`] call when
    /// acquire already built destinations (avoids decrypting S3 secrets twice).
    pub fn listing_backend(&self) -> Result<Box<dyn StorageBackend>> {
        let backends = self
            .items
            .iter()
            .map(|dest| dest.backend.clone_box())
            .collect::<Vec<_>>();
        if backends.len() == 1 {
            let mut backends = backends;
            return Ok(backends.remove(0));
        }
        Ok(Box::new(FanoutBackend::new(backends)?))
    }

    /// Into listing backend.
    pub fn into_listing_backend(self) -> Result<Box<dyn StorageBackend>> {
        let backends = self
            .items
            .into_iter()
            .map(|dest| dest.backend)
            .collect::<Vec<_>>();
        if backends.len() == 1 {
            let mut backends = backends;
            return Ok(backends.remove(0));
        }
        Ok(Box::new(FanoutBackend::new(backends)?))
    }
}
