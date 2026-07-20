//! Liberate pipeline: license → download → decrypt → storage.

use std::path::PathBuf;

use libation_audible::{fetch_and_download, DownloadOptions};
use libation_config::DownloadFormat;
use libation_decrypt::{aaxclean_available, decrypt_with_aaxclean, DecryptRequest};
use libation_library::{LiberateStatus, LibraryStore};
use libation_storage::{ObjectMeta, StorageBackend};
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
    /// Root for auth files (`LIBATION_FILES_DIR`).
    pub files_dir: PathBuf,
    /// Scratch directory for encrypted + decrypted temps.
    pub cache_dir: PathBuf,
    /// Optional override for `aaxclean-cli`.
    pub aaxclean_bin: Option<PathBuf>,
}

/// Result after a successful liberate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiberateResult {
    pub asin: String,
    pub storage_key: String,
}

/// Run the liberate pipeline for one book.
pub async fn liberate_book(
    library: &LibraryStore,
    storage: &dyn StorageBackend,
    req: LiberateRequest,
) -> Result<LiberateResult> {
    tracing::info!(asin = %req.asin, title = %req.title, "liberate requested");

    library.set_liberate_status(
        &req.asin,
        &req.account_id,
        LiberateStatus::Queued,
        None,
        None,
    )?;

    match run_pipeline(library, storage, &req).await {
        Ok(result) => {
            library.set_liberate_status(
                &req.asin,
                &req.account_id,
                LiberateStatus::Liberated,
                Some(&result.storage_key),
                None,
            )?;
            Ok(result)
        }
        Err(err) => {
            let message = err.to_string();
            let _ = library.set_liberate_status(
                &req.asin,
                &req.account_id,
                LiberateStatus::Error,
                None,
                Some(&message),
            );
            Err(err)
        }
    }
}

async fn run_pipeline(
    library: &LibraryStore,
    storage: &dyn StorageBackend,
    req: &LiberateRequest,
) -> Result<LiberateResult> {
    library.set_liberate_status(
        &req.asin,
        &req.account_id,
        LiberateStatus::Downloading,
        None,
        None,
    )?;

    let work_dir = req.cache_dir.join("liberate").join(&req.asin);
    tokio::fs::create_dir_all(&work_dir).await?;

    let (_account, download, _summary) = fetch_and_download(
        &req.files_dir,
        &req.account_id,
        &req.asin,
        req.options.quality,
        &work_dir,
    )
    .await?;

    let liberated_path = if download.needs_decrypt {
        let (Some(key), Some(iv)) = (&download.key, &download.iv) else {
            return Err(LiberateError::Other(anyhow::anyhow!(
                "aaxc download missing key/iv"
            )));
        };
        if !aaxclean_available(req.aaxclean_bin.as_deref()).await {
            return Err(LiberateError::Decrypt(
                libation_decrypt::DecryptError::AaxcleanNotFound(
                    req.aaxclean_bin
                        .clone()
                        .unwrap_or_else(|| std::path::PathBuf::from("aaxclean-cli")),
                ),
            ));
        }
        let out = work_dir.join(format!("{}.m4b", req.asin));
        decrypt_with_aaxclean(DecryptRequest {
            input: download.path.clone(),
            output: out.clone(),
            audible_key: Some(key.clone()),
            audible_iv: Some(iv.clone()),
            activation_bytes: None,
            aaxclean_bin: req.aaxclean_bin.clone(),
        })
        .await?;
        out
    } else {
        download.path.clone()
    };

    let ext = liberated_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or(match req.options.format {
            DownloadFormat::M4b => "m4b",
            DownloadFormat::Mp3 => "mp3",
        });

    if matches!(req.options.format, DownloadFormat::Mp3) && !ext.eq_ignore_ascii_case("mp3") {
        tracing::warn!(
            asin = %req.asin,
            got = %ext,
            "download.format=mp3 requested but liberate produced non-mp3; re-encode is not supported yet — storing as-is"
        );
    }

    let storage_key = default_storage_key(
        req.authors.as_deref(),
        &req.title,
        &req.asin,
        ext,
    );

    let data_len = tokio::fs::metadata(&liberated_path)
        .await
        .map(|m| m.len())
        .ok();
    let meta = ObjectMeta {
        content_type: Some(content_type_for_ext(ext).into()),
        content_length: data_len,
        asin: Some(req.asin.clone()),
        title: Some(req.title.clone()),
    };
    storage.put_file(&storage_key, &liberated_path, meta).await?;

    // Best-effort cleanup of scratch files (including key material on disk).
    if let Err(err) = tokio::fs::remove_dir_all(&work_dir).await {
        tracing::warn!(
            path = %work_dir.display(),
            error = %err,
            "failed to clean liberate cache dir"
        );
    }

    Ok(LiberateResult {
        asin: req.asin.clone(),
        storage_key,
    })
}

fn content_type_for_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "m4b" => "audio/mp4",
        "aaxc" | "aax" => "audio/vnd.audible.aax",
        _ => "application/octet-stream",
    }
}

/// Compute the storage key that would be used (for dry-run / set-status).
#[must_use]
pub fn planned_storage_key(req: &LiberateRequest) -> String {
    default_storage_key(
        req.authors.as_deref(),
        &req.title,
        &req.asin,
        match req.options.format {
            DownloadFormat::M4b => "m4b",
            DownloadFormat::Mp3 => "mp3",
        },
    )
}
