//! Recommendation engine: storefront candidates scored against local taste.
//!
//! Flow:
//! 1. Build local taste seeds (finished / rated / listening)
//! 2. Expand **unowned** candidates from storefronts (Libro, Audible, Chirp, GA)
//! 3. Score those candidates with local signals + embedding similarity
//!    (series completion + active listening are first-class heuristics)
//! 4. Merge open title requests; attach purchase hints

use std::collections::{HashMap, HashSet};

use bookclerk_library::{
    AcquireStatus, BookRecord, LibraryStore, ListeningProgressRecord, RequestStatus,
};

use crate::candidates::{
    gather_storefront_candidates, select_taste_seeds, CandidateFetchOptions, StorefrontCandidate,
};
use crate::embed::{bytes_to_vector, cosine, open_embedder, Embedder};
use crate::error::Result;
use crate::purchase::{purchase_hints_for, PurchaseHint};

/// Tunables for [`recommend`].
#[derive(Debug, Clone)]
pub struct RecommendOptions {
    pub limit: usize,
    pub embedding_model: String,
    pub region: String,
    pub include_purchase_hints: bool,
    /// When set, only listening rows for this external user influence ranking.
    pub external_user_id: Option<String>,
    /// Pull unowned titles from storefront catalogs (the primary path).
    pub fetch_storefront_candidates: bool,
    pub storefront_seed_limit: usize,
    pub storefront_max_remote_calls: usize,
    /// Drop GraphicAudio Magento series-set SKUs from discovery candidates.
    pub exclude_graphicaudio_series_sets: bool,
    /// Shelf kinds / ids to hide (`finish_series`, `author`, `genre`, `from_store`, …).
    /// Empty = offer every shelf Discover can build.
    pub disabled_shelves: Vec<String>,
    /// Models dir for on-the-fly candidate embedding (optional; empty = skip).
    pub models_dir: Option<std::path::PathBuf>,
    pub embed_intra_threads: usize,
    pub embeddings_enabled: bool,
}

impl Default for RecommendOptions {
    fn default() -> Self {
        Self {
            limit: 20,
            embedding_model: String::from(crate::embed::default_embedding_model_id()),
            region: String::from("us"),
            include_purchase_hints: true,
            external_user_id: None,
            fetch_storefront_candidates: true,
            storefront_seed_limit: 8,
            storefront_max_remote_calls: 32,
            exclude_graphicaudio_series_sets: false,
            disabled_shelves: Vec::new(),
            models_dir: None,
            embed_intra_threads: 1,
            embeddings_enabled: true,
        }
    }
}

/// One ranked recommendation (typically an unowned storefront title).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Recommendation {
    pub work_id: Option<String>,
    pub title: String,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    pub score: f64,
    pub reasons: Vec<String>,
    pub purchase_hints: Vec<PurchaseHint>,
    /// True when sourced from an open title request rather than storefront discovery.
    pub from_request: bool,
    pub request_uuid: Option<String>,
    /// Storefront that proposed this title (`audible`, `libro`, …).
    pub candidate_source: Option<String>,
    pub candidate_product_id: Option<String>,
    /// Categories/subjects copied from the taste seed that produced this hit.
    pub seed_categories: Option<String>,
}

/// Per-series local signal used for completion / listening heuristics.
#[derive(Debug, Clone, Default)]
struct SeriesAffinity {
    owned_count: usize,
    finished_count: usize,
    /// Owned books in this series that have listening activity.
    listening_count: usize,
    /// At least one in-progress (not finished) listen in this series.
    active_listening: bool,
    max_owned_index: Option<f64>,
}

/// Build ranked recommendations for the operator (or a specific ABS user).
pub async fn recommend(
    library: &LibraryStore,
    opts: &RecommendOptions,
) -> Result<Vec<Recommendation>> {
    let mut recs = recommend_all(library, opts).await?;
    recs.truncate(opts.limit);
    Ok(recs)
}

/// Personalized Discover feed (Netflix-style shelves) from the same candidate pool.
pub async fn recommend_feed(
    library: &LibraryStore,
    opts: &RecommendOptions,
) -> Result<crate::shelves::DiscoverFeed> {
    let recs = recommend_all(library, opts).await?;
    let books = library.list_books(None)?;
    let listening = library.list_listening_progress(opts.external_user_id.as_deref())?;
    let taste = build_shelf_taste(&books, &listening);
    Ok(crate::shelves::build_discover_feed(
        &recs,
        &taste,
        opts.limit.clamp(6, 12),
        &opts.disabled_shelves,
    ))
}

/// Full scored candidate pool (not truncated) used by flat list + shelves.
async fn recommend_all(
    library: &LibraryStore,
    opts: &RecommendOptions,
) -> Result<Vec<Recommendation>> {
    let books = library.list_books(None)?;

    let owned_asins: HashSet<String> = books
        .iter()
        .filter_map(|b| b.asin.clone())
        .map(|s| s.to_ascii_uppercase())
        .collect();
    let owned_isbns: HashSet<String> = books.iter().filter_map(|b| b.isbn.clone()).collect();
    let owned_product_keys: HashSet<String> = books
        .iter()
        .flat_map(|b| {
            [
                format!("{}:{}", b.source, b.product_id),
                b.product_id.clone(),
            ]
        })
        .collect();

    let mut liked_authors: HashMap<String, f64> = HashMap::new();
    let mut liked_narrators: HashMap<String, f64> = HashMap::new();
    let mut liked_categories: HashMap<String, f64> = HashMap::new();
    let mut seed_work_ids: HashSet<String> = HashSet::new();
    let mut listening_boost: HashSet<String> = HashSet::new();

    for book in &books {
        let mut weight = 0.0;
        if book.is_finished {
            weight += 3.0;
        }
        if let Some(r) = book.rating_overall {
            if r >= 4.0 {
                weight += 2.0;
            } else if r >= 3.0 {
                weight += 1.0;
            }
        }
        if book.acquire_status == AcquireStatus::Acquired {
            weight += 0.5;
        }
        if weight <= 0.0 {
            continue;
        }
        if let Ok(Some(wid)) = library.work_id_for_book(&book.uuid) {
            seed_work_ids.insert(wid);
        }
        if let Some(authors) = &book.authors {
            for a in split_tokens_display(authors) {
                *liked_authors.entry(a.to_lowercase()).or_default() += weight;
            }
        }
        if let Some(narrators) = &book.narrators {
            for n in split_tokens_display(narrators) {
                *liked_narrators.entry(n.to_lowercase()).or_default() += weight;
            }
        }
        if let Some(cats) = book.categories.as_ref().or(book.subjects.as_ref()) {
            for c in split_tokens_display(cats) {
                *liked_categories.entry(c.to_lowercase()).or_default() += weight;
            }
        }
    }

    let listening = library.list_listening_progress(opts.external_user_id.as_deref())?;
    for row in &listening {
        let weight = if row.is_finished {
            4.0
        } else if row.progress.unwrap_or(0.0) > 0.2 {
            2.0
        } else {
            0.5
        };
        if let Some(wid) = &row.work_id {
            seed_work_ids.insert(wid.clone());
        }
        if let Some(uuid) = &row.book_uuid {
            listening_boost.insert(uuid.clone());
        }
        if let Some(authors) = &row.authors {
            for a in split_tokens_display(authors) {
                *liked_authors.entry(a.to_lowercase()).or_default() += weight;
            }
        }
    }

    let series_affinity = build_series_affinity(&books, &listening, &listening_boost);
    let seeds = select_taste_seeds(&books, &listening_boost);

    // --- Primary path: storefront candidates not in the owned library ---
    let mut scored: HashMap<String, Recommendation> = HashMap::new();

    if opts.fetch_storefront_candidates && !seeds.is_empty() {
        let fetch_opts = CandidateFetchOptions {
            region: opts.region.clone(),
            seed_limit: opts.storefront_seed_limit,
            max_remote_calls: opts.storefront_max_remote_calls,
            exclude_graphicaudio_series_sets: opts.exclude_graphicaudio_series_sets,
            ..CandidateFetchOptions::default()
        };
        let candidates = gather_storefront_candidates(
            library,
            &seeds,
            &owned_asins,
            &owned_isbns,
            &owned_product_keys,
            &fetch_opts,
        )
        .await?;

        let seed_centroid =
            seed_embedding_centroid(library, &seed_work_ids, &opts.embedding_model)?;
        let mut embedder = open_candidate_embedder(opts)?;

        for c in candidates {
            let key = candidate_key(&c);
            let mut score = 1.0;
            let mut reasons = vec![c.origin.clone()];

            if let Some(authors) = &c.authors {
                for a in split_tokens_display(authors) {
                    let key = a.to_lowercase();
                    if let Some(w) = liked_authors.get(&key) {
                        score += w * 0.9;
                        reasons.push(format!("matches liked author ({a})"));
                    }
                }
            }
            if let Some(narrators) = &c.narrators {
                for n in split_tokens_display(narrators) {
                    let key = n.to_lowercase();
                    if let Some(w) = liked_narrators.get(&key) {
                        score += w * 0.7;
                        reasons.push(format!("matches liked narrator ({n})"));
                    }
                }
            }
            if let Some(cats) = &c.seed_categories {
                for cat in split_tokens_display(cats) {
                    let key = cat.to_lowercase();
                    if let Some(w) = liked_categories.get(&key) {
                        score += w * 0.45;
                        reasons.push(format!("matches liked category ({cat})"));
                    }
                }
            }

            apply_series_completion_score(
                &c.series,
                c.series_index.as_deref(),
                &series_affinity,
                &mut score,
                &mut reasons,
            );

            // Embedding similarity vs local taste centroid.
            if let (Some(centroid), Some(embedder)) = (&seed_centroid, embedder.as_mut()) {
                let text = candidate_embed_text(&c);
                if let Ok(vectors) = embedder.embed(&[text]) {
                    if let Some(v) = vectors.first() {
                        let sim = cosine(centroid, v);
                        if sim > 0.15 {
                            score += f64::from(sim) * 12.0;
                            reasons.push(format!("similar to titles you finish (sim {sim:.2})"));
                        }
                    }
                }
            }

            // Light boost for Chirp deal merchandising (still filtered by ownership).
            if c.origin.contains("chirp top deals") || c.origin.contains("chirp free deals") {
                score += 1.5;
            }

            scored.insert(
                key,
                Recommendation {
                    work_id: None,
                    title: c.title,
                    authors: c.authors,
                    narrators: c.narrators,
                    series: c.series,
                    series_index: c.series_index,
                    asin: c.asin.clone(),
                    isbn: c.isbn.clone(),
                    score,
                    reasons,
                    purchase_hints: Vec::new(),
                    from_request: false,
                    request_uuid: None,
                    candidate_source: Some(c.source),
                    candidate_product_id: Some(c.product_id),
                    seed_categories: c.seed_categories,
                },
            );
        }
    }

    // Open requests that are not already owned.
    for req in library.list_title_requests(Some(RequestStatus::Open))? {
        let owned = req
            .asin
            .as_ref()
            .map(|a| owned_asins.contains(&a.to_ascii_uppercase()))
            .unwrap_or(false)
            || req
                .isbn
                .as_ref()
                .map(|i| owned_isbns.contains(i))
                .unwrap_or(false);
        if owned {
            continue;
        }
        let key = format!(
            "request:{}",
            req.asin
                .as_deref()
                .or(req.isbn.as_deref())
                .unwrap_or(req.uuid.as_str())
        );
        scored.insert(
            key,
            Recommendation {
                work_id: req.work_id,
                title: req.title,
                authors: req.authors,
                narrators: None,
                series: None,
                series_index: None,
                asin: req.asin,
                isbn: req.isbn,
                score: 14.0,
                reasons: vec![String::from("open title request")],
                purchase_hints: Vec::new(),
                from_request: true,
                request_uuid: Some(req.uuid),
                candidate_source: req.preferred_source,
                candidate_product_id: None,
                seed_categories: None,
            },
        );
    }

    let mut recs: Vec<Recommendation> = scored.into_values().collect();

    if opts.include_purchase_hints {
        attach_purchase_hints(&mut recs, opts).await;
    }

    // Sort flat list for callers that still want a single ranking.
    recs.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(recs)
}

async fn attach_purchase_hints(recs: &mut [Recommendation], opts: &RecommendOptions) {
    for rec in recs.iter_mut() {
        if let (Some(source), Some(pid)) = (
            rec.candidate_source.as_deref(),
            rec.candidate_product_id.as_deref(),
        ) {
            match source {
                "audible" => {
                    rec.purchase_hints.push(PurchaseHint {
                        source: String::from("audible"),
                        product_id: pid.to_string(),
                        title: Some(rec.title.clone()),
                        url: Some(format!(
                            "https://www.audible{}/pd/{}",
                            region_host_suffix(&opts.region),
                            pid.to_ascii_uppercase()
                        )),
                    });
                }
                "libro" => {
                    rec.purchase_hints.push(PurchaseHint {
                        source: String::from("libro"),
                        product_id: pid.to_string(),
                        title: Some(rec.title.clone()),
                        url: Some(format!("https://libro.fm/audiobooks/{pid}")),
                    });
                }
                "chirp" => {
                    rec.purchase_hints.push(PurchaseHint {
                        source: String::from("chirp"),
                        product_id: pid.to_string(),
                        title: Some(rec.title.clone()),
                        url: Some(format!("https://www.chirpbooks.com/audiobooks/{pid}")),
                    });
                }
                "graphicaudio" => {
                    rec.purchase_hints.push(PurchaseHint {
                        source: String::from("graphicaudio"),
                        product_id: pid.to_string(),
                        title: Some(rec.title.clone()),
                        url: Some(format!(
                            "https://www.graphicaudio.net/catalog/product/view/id/{pid}"
                        )),
                    });
                }
                _ => {}
            }
        }
        if rec.purchase_hints.is_empty() {
            match purchase_hints_for(
                &rec.title,
                rec.authors.as_deref(),
                rec.asin.as_deref(),
                rec.isbn.as_deref(),
                &opts.region,
            )
            .await
            {
                Ok(hints) => rec.purchase_hints = hints,
                Err(err) => tracing::debug!(error = %err, "purchase hint lookup failed"),
            }
        }
    }
}

fn build_shelf_taste(
    books: &[BookRecord],
    listening: &[ListeningProgressRecord],
) -> crate::shelves::ShelfTaste {
    let mut taste = crate::shelves::ShelfTaste::default();
    for book in books {
        if !book.source.trim().is_empty() {
            taste.owned_sources.insert(book.source.to_ascii_lowercase());
        }
        let mut weight = 0.0;
        if book.is_finished {
            weight += 3.0;
        }
        if let Some(r) = book.rating_overall {
            if r >= 4.0 {
                weight += 2.0;
            } else if r >= 3.0 {
                weight += 1.0;
            }
        }
        if weight <= 0.0 {
            continue;
        }
        if let Some(authors) = &book.authors {
            for a in split_tokens_display(authors) {
                let key = a.to_lowercase();
                let entry = taste.authors.entry(key).or_insert((a.clone(), 0.0));
                entry.0 = a;
                entry.1 += weight;
            }
            taste
                .seed_authors_by_title
                .insert(book.title.to_lowercase(), split_tokens_display(authors));
        }
        if let Some(narrators) = &book.narrators {
            for n in split_tokens_display(narrators) {
                let key = n.to_lowercase();
                let entry = taste.narrators.entry(key).or_insert((n.clone(), 0.0));
                entry.0 = n;
                entry.1 += weight;
            }
        }
        if let Some(cats) = book.categories.as_ref().or(book.subjects.as_ref()) {
            for c in split_tokens_display(cats) {
                let key = c.to_lowercase();
                let entry = taste.categories.entry(key).or_insert((c.clone(), 0.0));
                entry.0 = c;
                entry.1 += weight;
            }
        }
    }
    for row in listening {
        let weight = if row.is_finished {
            4.0
        } else if row.progress.unwrap_or(0.0) > 0.2 {
            2.0
        } else {
            0.5
        };
        if let Some(authors) = &row.authors {
            for a in split_tokens_display(authors) {
                let key = a.to_lowercase();
                let entry = taste.authors.entry(key).or_insert((a.clone(), 0.0));
                entry.0 = a;
                entry.1 += weight;
            }
        }
    }
    taste
}

fn build_series_affinity(
    books: &[BookRecord],
    listening: &[ListeningProgressRecord],
    listening_boost: &HashSet<String>,
) -> HashMap<String, SeriesAffinity> {
    let mut by_series: HashMap<String, SeriesAffinity> = HashMap::new();
    let book_by_uuid: HashMap<&str, &BookRecord> =
        books.iter().map(|b| (b.uuid.as_str(), b)).collect();

    for book in books {
        let Some(series) = book
            .series
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let key = series.to_lowercase();
        let entry = by_series.entry(key).or_default();
        entry.owned_count += 1;
        if book.is_finished {
            entry.finished_count += 1;
        }
        if listening_boost.contains(&book.uuid) {
            entry.listening_count += 1;
        }
        if let Some(idx) = parse_series_index(book.series_index.as_deref()) {
            entry.max_owned_index = Some(match entry.max_owned_index {
                Some(m) => m.max(idx),
                None => idx,
            });
        }
    }

    // Mark active (in-progress) listening via progress rows linked to owned books.
    for row in listening {
        if row.is_finished {
            continue;
        }
        let in_progress =
            row.progress.unwrap_or(0.0) > 0.0 || row.current_time_seconds.unwrap_or(0.0) > 0.0;
        if !in_progress {
            continue;
        }
        let series = row
            .book_uuid
            .as_deref()
            .and_then(|u| book_by_uuid.get(u))
            .and_then(|b| b.series.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(series) = series else {
            continue;
        };
        let entry = by_series.entry(series.to_lowercase()).or_default();
        entry.active_listening = true;
    }

    by_series
}

/// Score an unowned candidate against incomplete-series / listening affinity.
fn apply_series_completion_score(
    candidate_series: &Option<String>,
    candidate_index: Option<&str>,
    affinity: &HashMap<String, SeriesAffinity>,
    score: &mut f64,
    reasons: &mut Vec<String>,
) {
    let Some(series) = candidate_series
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let Some(aff) = affinity.get(&series.to_lowercase()) else {
        return;
    };
    if aff.owned_count == 0 {
        return;
    }

    // Owning some of the series and seeing an unowned sibling ⇒ complete the series.
    *score += 6.0 + (aff.owned_count.min(5) as f64 - 1.0) * 1.5;
    reasons.push(format!(
        "complete series (“{series}”; own {})",
        aff.owned_count
    ));

    if let Some(cand_idx) = parse_series_index(candidate_index) {
        if let Some(max_owned) = aff.max_owned_index {
            if cand_idx > max_owned && cand_idx <= max_owned + 1.5 {
                *score += 8.0;
                reasons.push(format!("next book in series (after {max_owned})"));
            } else if cand_idx > max_owned {
                *score += 3.0;
                reasons.push(format!("later book in series (#{cand_idx})"));
            }
        }
    }

    if aff.active_listening {
        *score += 5.0;
        reasons.push(format!("series “{series}” actively being listened to"));
    }
    if aff.listening_count >= 2 {
        *score += 4.0;
        reasons.push(format!(
            "multiple books in “{series}” being listened to ({})",
            aff.listening_count
        ));
    } else if aff.listening_count == 1 {
        *score += 2.0;
        reasons.push(format!("listening activity in “{series}”"));
    }
}

/// Parse Audible/Chirp-style series indexes (`"1"`, `"1.5"`, `"Book 2"`, `"02"`).
#[must_use]
pub fn parse_series_index(raw: Option<&str>) -> Option<f64> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(n) = raw.parse::<f64>() {
        return Some(n);
    }
    // Pull the first number-like token.
    let mut num = String::new();
    let mut seen_digit = false;
    let mut seen_dot = false;
    for c in raw.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            seen_digit = true;
        } else if c == '.' && seen_digit && !seen_dot {
            num.push(c);
            seen_dot = true;
        } else if seen_digit {
            break;
        }
    }
    if num.is_empty() || num == "." {
        None
    } else {
        num.parse().ok()
    }
}

fn candidate_key(c: &StorefrontCandidate) -> String {
    c.asin
        .as_deref()
        .map(|a| format!("asin:{}", a.to_ascii_uppercase()))
        .or_else(|| c.isbn.as_deref().map(|i| format!("isbn:{i}")))
        .unwrap_or_else(|| format!("{}:{}", c.source, c.product_id))
}

fn candidate_embed_text(c: &StorefrontCandidate) -> String {
    let mut parts = vec![c.title.clone()];
    if let Some(a) = &c.authors {
        parts.push(a.clone());
    }
    if let Some(n) = &c.narrators {
        parts.push(n.clone());
    }
    if let Some(s) = &c.series {
        parts.push(s.clone());
    }
    parts.join("\n")
}

fn seed_embedding_centroid(
    library: &LibraryStore,
    seed_work_ids: &HashSet<String>,
    model: &str,
) -> Result<Option<Vec<f32>>> {
    let mut query: Option<Vec<f32>> = None;
    let mut count = 0usize;
    for wid in seed_work_ids {
        if let Some((_, blob)) = library.get_embedding_vector("work", wid, model)? {
            let v = bytes_to_vector(&blob);
            match &mut query {
                Some(acc) => {
                    if acc.len() == v.len() {
                        for i in 0..acc.len() {
                            acc[i] += v[i];
                        }
                        count += 1;
                    }
                }
                None => {
                    query = Some(v);
                    count = 1;
                }
            }
        }
    }
    if let Some(mut q) = query {
        if count > 0 {
            for x in &mut q {
                *x /= count as f32;
            }
        }
        Ok(Some(q))
    } else {
        Ok(None)
    }
}

fn open_candidate_embedder(opts: &RecommendOptions) -> Result<Option<Box<dyn Embedder>>> {
    if !opts.embeddings_enabled {
        return Ok(None);
    }
    let Some(dir) = &opts.models_dir else {
        // Still allow hash embedder without a models dir.
        return Ok(Some(Box::new(crate::embed::HashEmbedder::new(384))));
    };
    Ok(Some(open_embedder(
        dir,
        opts.embed_intra_threads,
        opts.embeddings_enabled,
    )?))
}

fn region_host_suffix(region: &str) -> &'static str {
    match region {
        "uk" => ".co.uk",
        "ca" => ".ca",
        "au" => ".com.au",
        "fr" => ".fr",
        "de" => ".de",
        "jp" => ".co.jp",
        "it" => ".it",
        "in" => ".in",
        "es" => ".es",
        _ => ".com",
    }
}

fn split_tokens_display(s: &str) -> Vec<String> {
    s.split([',', ';', '/', '|', '&'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_series_index_variants() {
        assert_eq!(parse_series_index(Some("1")), Some(1.0));
        assert_eq!(parse_series_index(Some("1.5")), Some(1.5));
        assert_eq!(parse_series_index(Some("Book 3")), Some(3.0));
        assert_eq!(parse_series_index(Some("02")), Some(2.0));
        assert_eq!(parse_series_index(Some("")), None);
        assert_eq!(parse_series_index(None), None);
    }

    #[test]
    fn series_completion_boosts_next_and_listening() {
        let mut affinity = HashMap::new();
        affinity.insert(
            String::from("mistborn"),
            SeriesAffinity {
                owned_count: 2,
                finished_count: 1,
                listening_count: 2,
                active_listening: true,
                max_owned_index: Some(2.0),
            },
        );
        let mut score = 1.0;
        let mut reasons = Vec::new();
        apply_series_completion_score(
            &Some(String::from("Mistborn")),
            Some("3"),
            &affinity,
            &mut score,
            &mut reasons,
        );
        assert!(
            score > 20.0,
            "expected strong completion+listening score, got {score}"
        );
        assert!(reasons.iter().any(|r| r.contains("complete series")));
        assert!(reasons.iter().any(|r| r.contains("next book")));
        assert!(reasons
            .iter()
            .any(|r| r.contains("actively being listened")));
        assert!(reasons.iter().any(|r| r.contains("multiple books")));
    }
}
