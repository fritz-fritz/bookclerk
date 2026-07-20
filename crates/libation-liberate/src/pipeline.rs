//! Liberate pipeline: license → download → decrypt → metadata → storage.

use std::path::{Path, PathBuf};

use libation_audible::{
    download_companion_pdf, download_cover_jpeg, download_licensed_audio,
    fetch_and_download_with_options, fetch_chapter_info, open_account_client, summarize_license,
    AccountClient, DownloadLicense, DrmKind, DownloadOptions,
};
use libation_config::DownloadFormat;
use libation_decrypt::{
    aaxclean_available, decrypt_cenc, decrypt_with_aaxclean, encode_to_mp3, ffmpeg_available,
    fixup_audiobook, CencDecryptRequest, DecryptRequest, FixupRequest,
};
use libation_library::{LiberateStatus, LibraryStore};
use libation_storage::{ObjectMeta, StorageBackend};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cue::{flatten_chapters, write_cue, write_ffmetadata};
use crate::error::{LiberateError, Result};
use crate::naming::{audio_basename, sidecar_key, storage_key, NamingContext};
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
    /// Pre-parsed license (classic `liberate --license`). Skips license API call.
    pub preloaded_license: Option<DownloadLicense>,
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

    if !req.force && !req.options.overwrite_existing {
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

    let (_account, download, _summary) = if let Some(license) = &req.preloaded_license {
        let account_client = open_account_client(&req.files_dir, &req.account_id).await?;
        let dest = work_dir.join(format!("{}.encrypted", req.asin));
        let download = download_licensed_audio(&account_client.client, license, &dest).await?;
        let summary = summarize_license(license);
        (account_client, download, summary)
    } else {
        fetch_and_download_with_options(
            &req.files_dir,
            &req.account_id,
            &req.asin,
            &req.options,
            &work_dir,
        )
        .await?
    };

    let account_client = _account;

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
        })
        .to_string();

    let storage_key = planned_storage_key_for(req, &ext);

    let need_chapters = req.options.create_cue
        || req.options.fixup_metadata
        || req.options.save_chapter_json;
    let chapter_info = if need_chapters {
        match fetch_chapter_info(
            &account_client.client,
            &account_client.marketplace,
            &req.asin,
            req.options.quality,
            &req.options.chapter_layout,
        )
        .await
        {
            Ok(info) => Some(info),
            Err(err) => {
                tracing::warn!(asin = %req.asin, error = %err, "chapter metadata fetch failed");
                None
            }
        }
    } else {
        None
    };
    let flat_chapters = chapter_info
        .as_ref()
        .map(flatten_chapters)
        .unwrap_or_default();

    let want_cover = req.options.download_cover || req.options.fixup_metadata;
    let cover_path = if want_cover {
        let dest = work_dir.join(format!("{}.cover.jpg", req.asin));
        match download_cover_jpeg(
            &account_client.client,
            &account_client.marketplace,
            &req.asin,
            &req.options.cover_size,
            &dest,
        )
        .await
        {
            Ok(path) => path,
            Err(err) => {
                tracing::warn!(asin = %req.asin, error = %err, "cover download failed");
                None
            }
        }
    } else {
        None
    };

    if req.options.fixup_metadata {
        let ffmeta_path = work_dir.join(format!("{}.chapters.ffmeta", req.asin));
        let ffmeta = if !flat_chapters.is_empty() {
            if let Err(err) = write_ffmetadata(
                &ffmeta_path,
                &req.title,
                req.authors.as_deref().unwrap_or("Unknown Author"),
                req.narrators.as_deref(),
                &flat_chapters,
                None,
            ) {
                tracing::warn!(asin = %req.asin, error = %err, "ffmetadata write failed");
                None
            } else {
                Some(ffmeta_path)
            }
        } else {
            None
        };

        let fixed = work_dir.join(format!("{}.fixed.{}", req.asin, ext));
        match fixup_audiobook(FixupRequest {
            input: liberated_path.clone(),
            output: fixed.clone(),
            title: req.title.clone(),
            author: req.authors.clone(),
            narrator: req.narrators.clone(),
            cover: cover_path.clone(),
            ffmetadata: ffmeta,
            ffmpeg_bin: req.ffmpeg_bin.clone(),
        })
        .await
        {
            Ok(outcome) => liberated_path = outcome.output,
            Err(err) => {
                tracing::warn!(
                    asin = %req.asin,
                    error = %err,
                    "metadata fixup failed; storing pre-fixup audio"
                );
            }
        }
    }

    let data_len = tokio::fs::metadata(&liberated_path)
        .await
        .map(|m| m.len())
        .ok();
    let meta = ObjectMeta {
        content_type: Some(content_type_for_ext(&ext).into()),
        content_length: data_len,
        asin: Some(req.asin.clone()),
        title: Some(req.title.clone()),
    };
    storage.put_file(&storage_key, &liberated_path, meta).await?;

    store_artifacts(
        storage,
        &ArtifactContext {
            req,
            account: &account_client,
            audio_key: &storage_key,
            work_dir: &work_dir,
            cover_path: cover_path.as_deref(),
            chapter_info: chapter_info.as_ref(),
            flat_chapters: &flat_chapters,
        },
    )
    .await;

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

struct ArtifactContext<'a> {
    req: &'a LiberateRequest,
    account: &'a AccountClient,
    audio_key: &'a str,
    work_dir: &'a Path,
    cover_path: Option<&'a Path>,
    chapter_info: Option<&'a Value>,
    flat_chapters: &'a [crate::cue::FlatChapter],
}

async fn store_artifacts(storage: &dyn StorageBackend, ctx: &ArtifactContext<'_>) {
    let req = ctx.req;
    let account = ctx.account;
    let audio_key = ctx.audio_key;
    let work_dir = ctx.work_dir;
    let cover_path = ctx.cover_path;
    let chapter_info = ctx.chapter_info;
    let flat_chapters = ctx.flat_chapters;

    if req.options.download_cover {
        if let Some(cover) = cover_path {
            let key = sidecar_key(audio_key, "jpg");
            let meta = sidecar_meta(&req.asin, &req.title, "image/jpeg", cover).await;
            if let Err(err) = storage.put_file(&key, cover, meta).await {
                tracing::warn!(asin = %req.asin, key = %key, error = %err, "cover store failed");
            }
        }
    }

    if req.options.create_cue && !flat_chapters.is_empty() {
        let cue_path = work_dir.join(format!("{}.cue", req.asin));
        let performer = req.authors.as_deref().unwrap_or("Unknown Author");
        match write_cue(
            &cue_path,
            &audio_basename(audio_key),
            performer,
            &req.title,
            flat_chapters,
        ) {
            Ok(()) => {
                let key = sidecar_key(audio_key, "cue");
                let meta = sidecar_meta(&req.asin, &req.title, "application/x-cue", &cue_path)
                    .await;
                if let Err(err) = storage.put_file(&key, &cue_path, meta).await {
                    tracing::warn!(asin = %req.asin, key = %key, error = %err, "cue store failed");
                }
            }
            Err(err) => {
                tracing::warn!(asin = %req.asin, error = %err, "cue write failed");
            }
        }
    }

    if req.options.save_chapter_json {
        if let Some(info) = chapter_info {
            let layout = chapter_layout_token(&req.options.chapter_layout);
            let json_path = work_dir.join(format!("{}.chapters.{layout}.json", req.asin));
            match tokio::fs::write(&json_path, serde_json::to_vec_pretty(info).unwrap_or_default())
                .await
            {
                Ok(()) => {
                    let key = sidecar_key(audio_key, &format!("chapters.{layout}.json"));
                    let meta =
                        sidecar_meta(&req.asin, &req.title, "application/json", &json_path).await;
                    if let Err(err) = storage.put_file(&key, &json_path, meta).await {
                        tracing::warn!(
                            asin = %req.asin,
                            key = %key,
                            error = %err,
                            "chapter json store failed"
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(asin = %req.asin, error = %err, "chapter json write failed");
                }
            }
        }
    }

    if req.options.download_pdf {
        let pdf_path = work_dir.join(format!("{}.pdf", req.asin));
        match download_companion_pdf(
            &account.client,
            &account.marketplace,
            &req.asin,
            &pdf_path,
        )
        .await
        {
            Ok(Some(path)) => {
                let key = sidecar_key(audio_key, "pdf");
                let meta = sidecar_meta(&req.asin, &req.title, "application/pdf", &path).await;
                if let Err(err) = storage.put_file(&key, &path, meta).await {
                    tracing::warn!(asin = %req.asin, key = %key, error = %err, "pdf store failed");
                }
            }
            Ok(None) => tracing::debug!(asin = %req.asin, "no companion PDF for title"),
            Err(err) => {
                tracing::warn!(asin = %req.asin, error = %err, "companion PDF download failed");
            }
        }
    }
}

async fn sidecar_meta(asin: &str, title: &str, content_type: &str, path: &Path) -> ObjectMeta {
    let content_length = tokio::fs::metadata(path).await.ok().map(|m| m.len());
    ObjectMeta {
        content_type: Some(content_type.into()),
        content_length,
        asin: Some(asin.to_string()),
        title: Some(title.to_string()),
    }
}

fn chapter_layout_token(layout: &str) -> &'static str {
    match layout.to_ascii_lowercase().as_str() {
        "flat" => "flat",
        _ => "tree",
    }
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

/// Download and store companion PDF only (classic `liberate --pdf`).
pub async fn liberate_pdf_only(
    library: &LibraryStore,
    storage: &dyn StorageBackend,
    req: &LiberateRequest,
) -> Result<LiberateResult> {
    let audio_key = planned_storage_key(req);
    let pdf_key = sidecar_key(&audio_key, "pdf");

    if !req.force {
        if let Some(book) = library.get_book(&req.asin, &req.account_id)? {
            if book.pdf_status == LiberateStatus::Liberated {
                if let Some(key) = book.pdf_storage_key {
                    return Ok(LiberateResult {
                        asin: req.asin.clone(),
                        storage_key: key,
                        matched_existing: true,
                    });
                }
            }
        }
    }

    let work_dir = req.cache_dir.join("liberate-pdf").join(&req.asin);
    tokio::fs::create_dir_all(&work_dir).await?;
    let account = libation_audible::open_account_client(&req.files_dir, &req.account_id).await?;
    let pdf_path = work_dir.join(format!("{}.pdf", req.asin));

    let Some(path) = libation_audible::download_companion_pdf(
        &account.client,
        &account.marketplace,
        &req.asin,
        &pdf_path,
    )
    .await?
    else {
        return Err(LiberateError::Other(anyhow::anyhow!(
            "no companion PDF available for {}",
            req.asin
        )));
    };

    let meta = sidecar_meta(&req.asin, &req.title, "application/pdf", &path).await;
    storage.put_file(&pdf_key, &path, meta).await?;
    library.set_pdf_status(
        &req.asin,
        &req.account_id,
        LiberateStatus::Liberated,
        Some(&pdf_key),
    )?;

    let _ = tokio::fs::remove_dir_all(&work_dir).await;
    Ok(LiberateResult {
        asin: req.asin.clone(),
        storage_key: pdf_key,
        matched_existing: false,
    })
}
