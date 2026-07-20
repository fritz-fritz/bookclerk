//! Liberate pipeline: license → download → decrypt → (optional mp3) → storage.

use std::path::PathBuf;

use libation_audible::{fetch_and_download_with_options, DrmKind, DownloadOptions};
use libation_config::DownloadFormat;
use libation_decrypt::{
    aaxclean_available, decrypt_cenc, decrypt_with_aaxclean, encode_to_mp3, ffmpeg_available,
    CencDecryptRequest, DecryptRequest,
};
use libation_library::{LiberateStatus, LibraryStore};
use libation_storage::{ObjectMeta, StorageBackend};
use serde::{Deserialize, Serialize};

use crate::error::{LiberateError, Result};
use crate::naming::{storage_key, NamingContext};
use crate::reconcile::{find_existing_for_request, StorageIndex};

/// Request to liberate a single title.
#[derive(Debug, Clone)]
pub struct LiberateRequest {
    pub asin: String,
    pub account_id: String,
    pub title: String,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub options: DownloadOptions,
    /// Root for auth files (`LIBATION_FILES_DIR`).
    pub files_dir: PathBuf,
    /// Scratch directory for encrypted + decrypted temps.
    pub cache_dir: PathBuf,
    /// Optional override for `aaxclean-cli`.
    pub aaxclean_bin: Option<PathBuf>,
    /// Optional override for `ffmpeg`.
    pub ffmpeg_bin: Option<PathBuf>,
    /// When true, download even if matching media already exists in storage.
    pub force: bool,
}

/// Result after a successful liberate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiberateResult {
    pub asin: String,
    pub storage_key: String,
    /// True when an existing file was matched and no download ran.
    pub matched_existing: bool,
}

/// Run the liberate pipeline for one book.
pub async fn liberate_book(
    library: &LibraryStore,
    storage: &dyn StorageBackend,
    req: LiberateRequest,
) -> Result<LiberateResult> {
    liberate_book_indexed(library, storage, req, None).await
}

/// Liberate with an optional pre-built [`StorageIndex`] (avoids re-listing storage
/// when liberating many titles). On success, newly written keys are inserted into
/// the index so later books in the same batch can match them.
pub async fn liberate_book_indexed(
    library: &LibraryStore,
    storage: &dyn StorageBackend,
    req: LiberateRequest,
    mut index: Option<&mut StorageIndex>,
) -> Result<LiberateResult> {
    tracing::info!(asin = %req.asin, title = %req.title, force = req.force, "liberate requested");

    if !req.force {
        let owned_index;
        let lookup = match index.as_deref() {
            Some(idx) => idx,
            None => {
                owned_index = StorageIndex::from_storage(storage).await?;
                &owned_index
            }
        };
        if let Some(key) = find_existing_for_request(lookup, &req) {
            tracing::info!(
                asin = %req.asin,
                key = %key,
                "skipping download — matched existing liberated media"
            );
            library.set_liberate_status(
                &req.asin,
                &req.account_id,
                LiberateStatus::Liberated,
                Some(&key),
                None,
            )?;
            return Ok(LiberateResult {
                asin: req.asin,
                storage_key: key,
                matched_existing: true,
            });
        }
    }

    library.set_liberate_status(
        &req.asin,
        &req.account_id,
        LiberateStatus::Queued,
        None,
        None,
    )?;

    match run_pipeline(library, storage, &req).await {
        Ok(result) => {
            if let Some(idx) = index.as_mut() {
                idx.insert_key(result.storage_key.clone());
            }
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

    let (_account, download, _summary) = fetch_and_download_with_options(
        &req.files_dir,
        &req.account_id,
        &req.asin,
        &req.options,
        &work_dir,
    )
    .await?;

    let mut liberated_path = if download.needs_decrypt {
        match download.drm_kind {
            DrmKind::Adrm => {
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
                                .unwrap_or_else(|| PathBuf::from("aaxclean-cli")),
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
            }
            DrmKind::Widevine => {
                let (Some(kid), Some(key)) = (&download.kid, &download.cenc_key) else {
                    return Err(LiberateError::Other(anyhow::anyhow!(
                        "Widevine download missing kid/key"
                    )));
                };
                let out = work_dir.join(format!("{}.m4b", req.asin));
                decrypt_cenc(CencDecryptRequest {
                    input: download.path.clone(),
                    output: out.clone(),
                    kid: kid.clone(),
                    key: key.clone(),
                    aaxclean_bin: req.aaxclean_bin.clone(),
                    ffmpeg_bin: req.ffmpeg_bin.clone(),
                })
                .await?;
                out
            }
            DrmKind::Mpeg => download.path.clone(),
        }
    } else {
        download.path.clone()
    };

    // Optional lossy re-encode (classic DecryptToLossy / format=mp3).
    if matches!(req.options.format, DownloadFormat::Mp3) {
        let ext = liberated_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if !ext.eq_ignore_ascii_case("mp3") {
            if !ffmpeg_available(req.ffmpeg_bin.as_deref()).await {
                return Err(LiberateError::Decrypt(
                    libation_decrypt::DecryptError::FfmpegNotFound(
                        req.ffmpeg_bin
                            .clone()
                            .unwrap_or_else(|| PathBuf::from("ffmpeg")),
                    ),
                ));
            }
            let mp3_out = work_dir.join(format!("{}.mp3", req.asin));
            encode_to_mp3(
                &liberated_path,
                &mp3_out,
                req.ffmpeg_bin.as_deref(),
            )
            .await?;
            liberated_path = mp3_out;
        }
    }

    let ext = liberated_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or(match req.options.format {
            DownloadFormat::M4b => "m4b",
            DownloadFormat::Mp3 => "mp3",
        });

    let storage_key = planned_storage_key_for(req, ext);

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
        matched_existing: false,
    })
}

fn content_type_for_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "m4b" => "audio/mp4",
        "aaxc" | "aax" | "cenc" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

fn naming_ctx(req: &LiberateRequest) -> NamingContext {
    NamingContext {
        asin: req.asin.clone(),
        title: req.title.clone(),
        authors: req.authors.clone(),
        narrators: req.narrators.clone(),
        series: req.series.clone(),
        series_index: req.series_index.clone(),
        account_id: Some(req.account_id.clone()),
    }
}

fn planned_storage_key_for(req: &LiberateRequest, ext: &str) -> String {
    storage_key(
        &naming_ctx(req),
        req.options.folder_template.as_deref(),
        req.options.file_template.as_deref(),
        ext,
    )
}

/// Compute the storage key that would be used (for dry-run / set-status).
#[must_use]
pub fn planned_storage_key(req: &LiberateRequest) -> String {
    planned_storage_key_for(
        req,
        match req.options.format {
            DownloadFormat::M4b => "m4b",
            DownloadFormat::Mp3 => "mp3",
        },
    )
}
