//! Match existing liberated media in storage to library rows.

use std::collections::HashMap;

use libation_config::DownloadFormat;
use libation_library::{BookRecord, LiberateStatus, LibraryStore};
use libation_storage::StorageBackend;

use crate::error::Result;
use crate::naming::{default_storage_key, storage_key, NamingContext};
use crate::pipeline::LiberateRequest;

/// Index of storage object keys, keyed by ASIN found in the path.
#[derive(Debug, Default, Clone)]
pub struct StorageIndex {
    /// ASIN (uppercase) → candidate storage keys.
    by_asin: HashMap<String, Vec<String>>,
    /// All keys (for planned-key exact checks).
    all_keys: HashMap<String, ()>,
}

impl StorageIndex {
    /// Build an index by listing the storage backend.
    pub async fn from_storage(storage: &dyn StorageBackend) -> Result<Self> {
        let objects = storage.list("").await?;
        let mut index = Self::default();
        for obj in objects {
            index.insert_key(obj.key);
        }
        Ok(index)
    }

    /// Insert a storage key into the index (test helper / incremental).
    pub fn insert_key(&mut self, key: String) {
        self.all_keys.insert(key.clone(), ());
        for asin in extract_asins_from_key(&key) {
            self.by_asin.entry(asin).or_default().push(key.clone());
        }
    }

    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.all_keys.contains_key(key)
    }

    /// Best matching storage key for an ASIN, if any.
    #[must_use]
    pub fn best_key_for_asin(&self, asin: &str) -> Option<&str> {
        let candidates = self.by_asin.get(&asin.to_ascii_uppercase())?;
        pick_best_media_key(candidates)
    }
}

/// Summary of a reconcile run.
#[derive(Debug, Clone, Default)]
pub struct ReconcileSummary {
    pub matched: u32,
    pub cleared: u32,
    pub unchanged: u32,
}

/// Options for [`reconcile_library`] and [`set_download_status`].
#[derive(Debug, Clone)]
pub struct ReconcileOptions {
    pub account: Option<String>,
    /// When true, books marked Liberated whose file is missing become NotLiberated.
    pub clear_missing: bool,
    /// Limit to these ASINs (empty = all).
    pub asins: Vec<String>,
    /// When true, only mark found files as Liberated (do not clear missing).
    pub only_mark_found: bool,
    /// When true, only clear Liberated rows with no matching file.
    pub only_clear_missing: bool,
}

impl Default for ReconcileOptions {
    fn default() -> Self {
        Self {
            account: None,
            clear_missing: true,
            asins: Vec::new(),
            only_mark_found: false,
            only_clear_missing: false,
        }
    }
}

/// Scan storage and update library liberate status / storage_key for matches.
pub async fn reconcile_library(
    library: &LibraryStore,
    storage: &dyn StorageBackend,
    options: ReconcileOptions,
) -> Result<ReconcileSummary> {
    let index = StorageIndex::from_storage(storage).await?;
    let books = library.list_books(options.account.as_deref())?;
    let mut summary = ReconcileSummary::default();

    for book in books {
        if !options.asins.is_empty()
            && !options
                .asins
                .iter()
                .any(|a| a.eq_ignore_ascii_case(&book.asin))
        {
            continue;
        }
        let matched = find_existing_for_book(&index, &book);
        match matched {
            Some(key) => {
                if options.only_clear_missing {
                    summary.unchanged += 1;
                    continue;
                }
                let needs_update = book.liberate_status != LiberateStatus::Liberated
                    || book.storage_key.as_deref() != Some(key.as_str());
                if needs_update {
                    library.set_liberate_status(
                        &book.asin,
                        &book.account_id,
                        LiberateStatus::Liberated,
                        Some(&key),
                        None,
                    )?;
                    summary.matched += 1;
                    tracing::info!(
                        asin = %book.asin,
                        key = %key,
                        "matched existing liberated media"
                    );
                } else {
                    summary.unchanged += 1;
                }
            }
            None => {
                if options.only_mark_found {
                    summary.unchanged += 1;
                    continue;
                }
                if options.clear_missing && book.liberate_status == LiberateStatus::Liberated {
                    library.set_liberate_status(
                        &book.asin,
                        &book.account_id,
                        LiberateStatus::NotLiberated,
                        None,
                        None,
                    )?;
                    summary.cleared += 1;
                } else {
                    summary.unchanged += 1;
                }
            }
        }
    }

    Ok(summary)
}

/// Find an existing liberated file for a book (planned path or ASIN-in-path).
#[must_use]
pub fn find_existing_for_book(index: &StorageIndex, book: &BookRecord) -> Option<String> {
    // 1. Exact stored key.
    if let Some(key) = &book.storage_key {
        if index.contains_key(key) {
            return Some(key.clone());
        }
    }

    // 2. Planned default naming (common extensions).
    for ext in planned_extensions() {
        let key = default_storage_key(
            book.authors.as_deref(),
            &book.title,
            &book.asin,
            ext,
        );
        if index.contains_key(&key) {
            return Some(key);
        }
    }

    // 3. Any object path containing this ASIN (classic Libation layouts, etc.).
    index
        .best_key_for_asin(&book.asin)
        .map(str::to_string)
}

/// Same as [`find_existing_for_book`] but for a liberate request before DB status is Liberated.
#[must_use]
pub fn find_existing_for_request(index: &StorageIndex, req: &LiberateRequest) -> Option<String> {
    let ctx = NamingContext {
        asin: req.asin.clone(),
        title: req.title.clone(),
        authors: req.authors.clone(),
        narrators: req.narrators.clone(),
        series: req.series.clone(),
        series_index: req.series_index.clone(),
        account_id: Some(req.account_id.clone()),
    };
    let ext = match req.options.format {
        DownloadFormat::M4b => "m4b",
        DownloadFormat::Mp3 => "mp3",
    };
    let planned = storage_key(
        &ctx,
        req.options.folder_template.as_deref(),
        req.options.file_template.as_deref(),
        ext,
    );
    if index.contains_key(&planned) {
        return Some(planned);
    }
    for alt in planned_extensions() {
        if *alt == ext {
            continue;
        }
        let key = storage_key(
            &ctx,
            req.options.folder_template.as_deref(),
            req.options.file_template.as_deref(),
            alt,
        );
        if index.contains_key(&key) {
            return Some(key);
        }
    }
    // Also try default Author/Title/ASIN layout for older files.
    for alt in planned_extensions() {
        let key = default_storage_key(req.authors.as_deref(), &req.title, &req.asin, alt);
        if index.contains_key(&key) {
            return Some(key);
        }
    }
    index.best_key_for_asin(&req.asin).map(str::to_string)
}

fn planned_extensions() -> &'static [&'static str] {
    &["m4b", "m4a", "mp3"]
}

/// Extract Audible-like ASINs from a storage key / file path.
///
/// Matches:
/// - filename stem `B00EXAMPLE.m4b`
/// - bracket form `Title [B00EXAMPLE].m4b` (classic Libation)
/// - path segment containing a standalone ASIN token
#[must_use]
pub fn extract_asins_from_key(key: &str) -> Vec<String> {
    let mut found = Vec::new();
    let normalized = key.replace('\\', "/");

    // Bracket form: [B0...]
    for part in normalized.split(['[', ']']) {
        push_asin_candidate(&mut found, part);
    }

    for segment in normalized.split('/') {
        let stem = segment
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(segment);
        // Strip trailing " [ASIN]" already handled; also try whole stem.
        push_asin_candidate(&mut found, stem);
        // Tokens separated by space / underscore / hyphen.
        for token in stem.split(|c: char| c.is_whitespace() || c == '_' || c == '-') {
            push_asin_candidate(&mut found, token);
        }
    }

    found.sort();
    found.dedup();
    found
}

fn push_asin_candidate(out: &mut Vec<String>, raw: &str) {
    let trimmed = raw.trim();
    if looks_like_asin(trimmed) {
        out.push(trimmed.to_ascii_uppercase());
    }
}

/// Audible product ids are typically 10 chars starting with `B`.
fn looks_like_asin(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 10 || bytes.len() > 12 {
        return false;
    }
    if !matches!(bytes[0], b'B' | b'b') {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn pick_best_media_key(candidates: &[String]) -> Option<&str> {
    candidates
        .iter()
        .min_by_key(|key| media_rank(key))
        .map(String::as_str)
}

fn media_rank(key: &str) -> u8 {
    let ext = key
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "m4b" => 0,
        "m4a" => 1,
        "mp3" => 2,
        "aaxc" | "aax" => 9,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_asin_from_default_key() {
        let asins = extract_asins_from_key("Author/Title/B00EXAMPLE1.m4b");
        assert_eq!(asins, vec!["B00EXAMPLE1".to_string()]);
    }

    #[test]
    fn extracts_asin_from_bracket_form() {
        let asins = extract_asins_from_key("Author/Some Title [B0D186SQWV].m4b");
        assert!(asins.iter().any(|a| a == "B0D186SQWV"));
    }

    #[test]
    fn prefers_m4b_over_aaxc() {
        let mut index = StorageIndex::default();
        index.insert_key("x/B00EXAMPLE1.aaxc".into());
        index.insert_key("x/B00EXAMPLE1.m4b".into());
        assert_eq!(
            index.best_key_for_asin("B00EXAMPLE1"),
            Some("x/B00EXAMPLE1.m4b")
        );
    }

    #[test]
    fn rejects_short_tokens() {
        assert!(extract_asins_from_key("Author/Book.m4b").is_empty());
    }
}

#[cfg(test)]
mod reconcile_integration {
    use super::*;
    use bytes::Bytes;
    use libation_library::{LibraryStore, NewBook};
    use libation_storage::{LocalFsBackend, ObjectMeta, StorageBackend};
    use tempfile::tempdir;

    #[tokio::test]
    async fn matches_existing_file_by_asin_in_path() {
        let dir = tempdir().unwrap();
        let store_root = dir.path().join("books");
        let backend = LocalFsBackend::new(store_root).unwrap();
        backend
            .put(
                "Some Author/Cool Book [B00EXAMPLE1].m4b",
                Bytes::from_static(b"audio"),
                ObjectMeta::default(),
            )
            .await
            .unwrap();

        let library = LibraryStore::open_in_memory().unwrap();
        library
            .upsert_account("acct", "us", None, true)
            .unwrap();
        library
            .upsert_book(&NewBook {
                asin: "B00EXAMPLE1".into(),
                account_id: "acct".into(),
                marketplace: "us".into(),
                title: "Cool Book".into(),
                authors: Some("Some Author".into()),
                narrators: None,
                series: None,
                series_index: None,
                purchased_at: None,
            })
            .unwrap();

        let summary = reconcile_library(
            &library,
            &backend,
            ReconcileOptions {
                account: None,
                clear_missing: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(summary.matched, 1);
        let book = library.get_book("B00EXAMPLE1", "acct").unwrap().unwrap();
        assert_eq!(book.liberate_status, LiberateStatus::Liberated);
        assert!(book
            .storage_key
            .as_deref()
            .unwrap()
            .contains("B00EXAMPLE1"));
    }
}
