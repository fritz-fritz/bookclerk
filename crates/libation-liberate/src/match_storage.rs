//! Match liberated audio in storage to library rows via list + metadata probe.
//!
//! Unlike path-template reconciliation, this flow:
//! 1. Lists only audio objects (`.m4b` / `.mp3` / `.m4a`)
//! 2. Probes each object for identity (`asin` user-metadata / local meta sidecar)
//!    without downloading bodies
//! 3. Falls back to ASIN/ISBN tokens embedded in the object key
//! 4. Optionally relocates matched files (and accompanying sidecars) onto the
//!    configured naming-profile layout

use std::collections::{HashMap, HashSet};

use libation_audible::DownloadOptions;
use libation_library::{BookRecord, LiberateStatus, LibraryStore};
use libation_storage::{is_audio_key, libation_meta_sidecar_key, ObjectProbe, StorageBackend};
use tracing::{debug, info, warn};

use crate::error::{LiberateError, Result};
use crate::naming::sidecar_key;
use crate::pipeline::planned_storage_key;
use crate::reconcile::{extract_asins_from_key, request_from_book};

/// Known sidecar suffixes written next to a liberated audio file.
const SIDECAR_SUFFIXES: &[&str] = &[
    "jpg",
    "cue",
    "chapters.tree.json",
    "chapters.flat.json",
    "metadata.json",
    "clips.json",
    "pdf",
    "aaxc",
    "libation-meta.json",
];

/// Options for [`match_storage_to_library`].
#[derive(Debug, Clone)]
pub struct MatchStorageOptions {
    pub account: Option<String>,
    /// Clear Liberated when no matching audio is found.
    pub clear_missing: bool,
    /// Limit to these ASINs / ids (empty = all).
    pub asins: Vec<String>,
    /// When true, only mark found files as Liberated (do not clear missing).
    pub only_mark_found: bool,
    /// When true, only clear Liberated rows with no matching file.
    pub only_clear_missing: bool,
    /// Relocate matched audio (and sidecars) onto the configured template layout.
    pub fix_layout: bool,
    pub download: DownloadOptions,
}

impl Default for MatchStorageOptions {
    fn default() -> Self {
        Self {
            account: None,
            clear_missing: true,
            asins: Vec::new(),
            only_mark_found: false,
            only_clear_missing: false,
            fix_layout: false,
            download: DownloadOptions::default(),
        }
    }
}

/// Summary of a storage→library match run.
#[derive(Debug, Clone, Default)]
pub struct MatchStorageSummary {
    pub matched: u32,
    pub relocated: u32,
    pub cleared: u32,
    pub unchanged: u32,
    pub unmatched_files: u32,
}

/// List audio in storage, probe metadata (no body download), match to library.
pub async fn match_storage_to_library(
    library: &LibraryStore,
    storage: &dyn StorageBackend,
    options: MatchStorageOptions,
) -> Result<MatchStorageSummary> {
    let audio = storage.list_audio("").await?;
    let all_objects = storage.list("").await?;
    let all_keys: HashSet<String> = all_objects.into_iter().map(|o| o.key).collect();

    // identity (uppercase) → best audio key
    let mut by_id: HashMap<String, String> = HashMap::new();
    let mut probed_keys = HashSet::new();

    for obj in &audio {
        probed_keys.insert(obj.key.clone());
        let probe = match storage.probe(&obj.key).await {
            Ok(p) => p,
            Err(err) => {
                warn!(key = %obj.key, error = %err, "storage probe failed; using path tokens only");
                ObjectProbe {
                    key: obj.key.clone(),
                    size: obj.size,
                    ..Default::default()
                }
            }
        };
        let mut ids = Vec::new();
        if let Some(asin) = probe.meta.asin.as_deref() {
            ids.push(asin.to_ascii_uppercase());
        }
        ids.extend(extract_asins_from_key(&obj.key));
        for id in ids {
            match by_id.get(&id) {
                Some(existing) if media_rank(existing) <= media_rank(&obj.key) => {}
                _ => {
                    by_id.insert(id, obj.key.clone());
                }
            }
        }
    }

    let books = library.list_books(options.account.as_deref())?;
    let filter: HashSet<String> = options
        .asins
        .iter()
        .map(|a| a.to_ascii_uppercase())
        .collect();
    let mut summary = MatchStorageSummary::default();
    let mut claimed_keys: HashSet<String> = HashSet::new();

    for book in &books {
        if !filter.is_empty() {
            let ids = book_identity_tokens(book);
            if !ids.iter().any(|id| filter.contains(id)) {
                continue;
            }
        }

        let Some(mut key) = find_audio_for_book(book, &by_id, &all_keys) else {
            if options.only_mark_found {
                summary.unchanged += 1;
                continue;
            }
            if options.clear_missing && book.liberate_status == LiberateStatus::Liberated {
                library.set_liberate_status(
                    book.title_id(),
                    &book.account_id,
                    LiberateStatus::NotLiberated,
                    None,
                    None,
                )?;
                summary.cleared += 1;
            } else {
                summary.unchanged += 1;
            }
            continue;
        };

        if options.only_clear_missing {
            claimed_keys.insert(key);
            summary.unchanged += 1;
            continue;
        }

        claimed_keys.insert(key.clone());

        if options.fix_layout {
            let planned = planned_storage_key(library, &request_from_book(book, &options.download));
            if planned != key {
                match relocate_with_sidecars(storage, &all_keys, &key, &planned).await {
                    Ok(()) => {
                        debug!(from = %key, to = %planned, "relocated matched audio to template layout");
                        key = planned;
                        summary.relocated += 1;
                    }
                    Err(err) => {
                        warn!(
                            from = %key,
                            to = %planned,
                            error = %err,
                            "failed to relocate matched audio; keeping existing key"
                        );
                    }
                }
            }
        }

        let already = book.liberate_status == LiberateStatus::Liberated
            && book.storage_key.as_deref() == Some(key.as_str());
        if already {
            summary.unchanged += 1;
        } else {
            library.set_liberate_status(
                book.title_id(),
                &book.account_id,
                LiberateStatus::Liberated,
                Some(&key),
                None,
            )?;
            summary.matched += 1;
            info!(
                asin = %book.asin_or_isbn(),
                key = %key,
                "matched existing liberated media via storage probe"
            );
        }
    }

    summary.unmatched_files = probed_keys
        .difference(&claimed_keys)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);

    Ok(summary)
}

fn book_identity_tokens(book: &BookRecord) -> Vec<String> {
    let mut ids = Vec::new();
    for raw in [
        Some(book.title_id()),
        Some(book.download_product_id()),
        Some(book.asin_or_isbn()),
        book.asin.as_deref(),
        book.isbn.as_deref(),
        Some(book.product_id.as_str()),
        Some(book.uuid.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if !raw.is_empty() {
            ids.push(raw.to_ascii_uppercase());
        }
    }
    ids
}

fn find_audio_for_book(
    book: &BookRecord,
    by_id: &HashMap<String, String>,
    all_keys: &HashSet<String>,
) -> Option<String> {
    // Prefer an exact stored key when the object is still present.
    if let Some(key) = &book.storage_key {
        if all_keys.contains(key) && is_audio_key(key) {
            return Some(key.clone());
        }
    }
    for id in book_identity_tokens(book) {
        if let Some(key) = by_id.get(&id) {
            return Some(key.clone());
        }
    }
    None
}

async fn relocate_with_sidecars(
    storage: &dyn StorageBackend,
    all_keys: &HashSet<String>,
    from_audio: &str,
    to_audio: &str,
) -> Result<()> {
    if from_audio == to_audio {
        return Ok(());
    }
    if storage.exists(to_audio).await? {
        return Err(LiberateError::Storage(
            libation_storage::StorageError::InvalidKey(format!(
                "destination already exists: {to_audio}"
            )),
        ));
    }

    storage.rename(from_audio, to_audio).await?;

    let companions = accompanying_keys(all_keys, from_audio);
    for from_side in companions {
        if !storage.exists(&from_side).await.unwrap_or(false) {
            continue;
        }
        let to_side = remap_companion_key(from_audio, to_audio, &from_side);
        if to_side == from_side {
            continue;
        }
        if storage.exists(&to_side).await.unwrap_or(false) {
            warn!(
                from = %from_side,
                to = %to_side,
                "destination accompanying file already exists; skipping"
            );
            continue;
        }
        if let Err(err) = storage.rename(&from_side, &to_side).await {
            warn!(
                from = %from_side,
                to = %to_side,
                error = %err,
                "failed to relocate accompanying file"
            );
        }
    }
    Ok(())
}

/// Keys that share the audio stem (sidecars) plus the local meta sidecar.
fn accompanying_keys(all_keys: &HashSet<String>, audio_key: &str) -> Vec<String> {
    let stem = audio_key
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(audio_key);
    let prefix = format!("{stem}.");
    let mut out: Vec<String> = all_keys
        .iter()
        .filter(|k| k.as_str() != audio_key && k.starts_with(&prefix))
        .cloned()
        .collect();
    // Ensure known suffixes are attempted even if listing raced.
    for suffix in SIDECAR_SUFFIXES {
        let key = if *suffix == "libation-meta.json" {
            libation_meta_sidecar_key(audio_key)
        } else {
            sidecar_key(audio_key, suffix)
        };
        if key != audio_key && !out.iter().any(|k| k == &key) {
            out.push(key);
        }
    }
    out
}

fn remap_companion_key(from_audio: &str, to_audio: &str, companion: &str) -> String {
    let from_stem = from_audio
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(from_audio);
    let to_stem = to_audio
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(to_audio);
    if let Some(rest) = companion.strip_prefix(&format!("{from_stem}.")) {
        format!("{to_stem}.{rest}")
    } else {
        companion.to_string()
    }
}

fn media_rank(key: &str) -> u8 {
    let ext = key
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "m4b" => 0,
        "m4a" => 1,
        "mp3" => 2,
        _ if is_audio_key(key) => 3,
        _ => 9,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use libation_library::NewBook;
    use libation_storage::{LocalFsBackend, ObjectMeta};
    use tempfile::tempdir;

    #[tokio::test]
    async fn matches_via_object_meta_without_asin_in_path() {
        let dir = tempdir().unwrap();
        let backend = LocalFsBackend::new(dir.path().to_path_buf()).unwrap();
        backend
            .put(
                "Misc/random-name.m4b",
                Bytes::from_static(b"audio"),
                ObjectMeta {
                    asin: Some("B00EXAMPLE1".into()),
                    title: Some("Cool Book".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        backend
            .put(
                "Misc/random-name.jpg",
                Bytes::from_static(b"jpg"),
                ObjectMeta::default(),
            )
            .await
            .unwrap();

        let library = LibraryStore::open_in_memory().unwrap();
        library.upsert_account("acct", "us", None, true).unwrap();
        let book = NewBook::minimal("B00EXAMPLE1", "acct", "us", "Cool Book");
        library.upsert_book(&book).unwrap();

        let summary = match_storage_to_library(
            &library,
            &backend,
            MatchStorageOptions {
                fix_layout: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(summary.matched, 1);
        let book = library.get_book("B00EXAMPLE1", "acct").unwrap().unwrap();
        assert_eq!(book.storage_key.as_deref(), Some("Misc/random-name.m4b"));
    }

    #[tokio::test]
    async fn fix_layout_moves_audio_and_sidecars() {
        let dir = tempdir().unwrap();
        let backend = LocalFsBackend::new(dir.path().to_path_buf()).unwrap();
        backend
            .put(
                "Misc/random-name.m4b",
                Bytes::from_static(b"audio"),
                ObjectMeta {
                    asin: Some("B00EXAMPLE1".into()),
                    title: Some("Cool Book".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        backend
            .put(
                "Misc/random-name.jpg",
                Bytes::from_static(b"jpg"),
                ObjectMeta::default(),
            )
            .await
            .unwrap();
        backend
            .put(
                "Misc/random-name.cue",
                Bytes::from_static(b"cue"),
                ObjectMeta::default(),
            )
            .await
            .unwrap();

        let library = LibraryStore::open_in_memory().unwrap();
        library.upsert_account("acct", "us", None, true).unwrap();
        let mut book = NewBook::minimal("B00EXAMPLE1", "acct", "us", "Cool Book");
        book.authors = Some("Jane Doe".into());
        library.upsert_book(&book).unwrap();

        let summary = match_storage_to_library(
            &library,
            &backend,
            MatchStorageOptions {
                fix_layout: true,
                download: DownloadOptions {
                    naming_profile: libation_config::NamingProfile::Audiobookshelf,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(summary.matched, 1);
        assert_eq!(summary.relocated, 1);

        let book = library.get_book("B00EXAMPLE1", "acct").unwrap().unwrap();
        let key = book.storage_key.expect("storage key");
        assert!(key.ends_with(".m4b"), "{key}");
        assert!(backend.exists(&key).await.unwrap());
        assert!(!backend.exists("Misc/random-name.m4b").await.unwrap());
        let cover = sidecar_key(&key, "jpg");
        let cue = sidecar_key(&key, "cue");
        assert!(backend.exists(&cover).await.unwrap(), "missing {cover}");
        assert!(backend.exists(&cue).await.unwrap(), "missing {cue}");
        assert!(!backend.exists("Misc/random-name.jpg").await.unwrap());
        // Local meta sidecar should follow the audio stem.
        assert!(backend
            .exists(&libation_meta_sidecar_key(&key))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn matches_asin_token_in_path_without_meta() {
        let dir = tempdir().unwrap();
        let backend = LocalFsBackend::new(dir.path().to_path_buf()).unwrap();
        backend
            .put(
                "Old Layout/Cool Book [B00EXAMPLE1].m4b",
                Bytes::from_static(b"audio"),
                ObjectMeta::default(),
            )
            .await
            .unwrap();

        let library = LibraryStore::open_in_memory().unwrap();
        library.upsert_account("acct", "us", None, true).unwrap();
        library
            .upsert_book(&NewBook::minimal("B00EXAMPLE1", "acct", "us", "Cool Book"))
            .unwrap();

        let summary = match_storage_to_library(&library, &backend, MatchStorageOptions::default())
            .await
            .unwrap();
        assert_eq!(summary.matched, 1);
    }
}
