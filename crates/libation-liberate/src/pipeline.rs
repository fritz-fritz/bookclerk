//! Liberate pipeline entry points.

use libation_audible::DownloadOptions;
use libation_library::{LiberateStatus, LibraryStore};
use libation_storage::StorageBackend;
use serde::{Deserialize, Serialize};

use crate::error::{LiberateError, Result};
use crate::naming::default_storage_key;

/// Request to liberate a single title.
#[derive(Debug, Clone)]
pub struct LiberateRequest {
    pub asin: String,
    pub account_id: String,
    pub title: String,
    pub authors: Option<String>,
    pub options: DownloadOptions,
}

/// Result after a successful liberate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiberateResult {
    pub asin: String,
    pub storage_key: String,
}

/// Run the liberate pipeline for one book.
///
/// Scaffold: marks the book as queued/error with a clear not-implemented message
/// until download + decrypt wiring is complete.
pub async fn liberate_book(
    library: &LibraryStore,
    _storage: &dyn StorageBackend,
    req: LiberateRequest,
) -> Result<LiberateResult> {
    tracing::info!(asin = %req.asin, title = %req.title, "liberate requested");

    library
        .set_liberate_status(
            &req.asin,
            &req.account_id,
            LiberateStatus::Queued,
            None,
            None,
        )
        .await?;

    let _planned_key = default_storage_key(
        req.authors.as_deref(),
        &req.title,
        &req.asin,
        match req.options.format {
            libation_config::DownloadFormat::M4b => "m4b",
            libation_config::DownloadFormat::Mp3 => "mp3",
        },
    );

    // Pipeline stages (wired in liberate-pipeline todo):
    // 1. get license via libation-audible
    // 2. download encrypted file to cache
    // 3. decrypt via libation-decrypt (aaxclean-cli)
    // 4. metadata fixup
    // 5. storage.put(key, …)
    // 6. update liberate_status = Liberated

    library
        .set_liberate_status(
            &req.asin,
            &req.account_id,
            LiberateStatus::Error,
            None,
            Some("download/decrypt pipeline not yet implemented"),
        )
        .await?;

    Err(LiberateError::NotImplemented(req.asin))
}

/// Compute the storage key that would be used (for dry-run / set-status).
#[must_use]
pub fn planned_storage_key(req: &LiberateRequest) -> String {
    default_storage_key(
        req.authors.as_deref(),
        &req.title,
        &req.asin,
        match req.options.format {
            libation_config::DownloadFormat::M4b => "m4b",
            libation_config::DownloadFormat::Mp3 => "mp3",
        },
    )
}
