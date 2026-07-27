//! Recommendation engine: storefront candidates scored against local taste.
//!
//! Flow:
//! 1. Build local taste seeds (finished / rated / listening)
//! 2. Expand **unowned** candidates from storefronts (Libro related, Audible catalog)
//! 3. Score those candidates with local signals + embedding similarity
//! 4. Merge open title requests; attach purchase hints

use std::collections::{HashMap, HashSet};

use bookclerk_library::{AcquireStatus, LibraryStore, RequestStatus};

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
    /// Pull unowned titles from Audible / Libro catalogs (the primary path).
    pub fetch_storefront_candidates: bool,
    pub storefront_seed_limit: usize,
    pub storefront_max_remote_calls: usize,
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
            storefront_max_remote_calls: 24,
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
}

/// Build ranked recommendations for the operator (or a specific ABS user).
pub async fn recommend(
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

    let mut liked_authors: HashMap<String, f64> = HashMap::new();
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
            for a in split_tokens(authors) {
                *liked_authors.entry(a).or_default() += weight;
            }
        }
        if let Some(cats) = book.categories.as_ref().or(book.subjects.as_ref()) {
            for c in split_tokens(cats) {
                *liked_categories.entry(c).or_default() += weight;
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
            for a in split_tokens(authors) {
                *liked_authors.entry(a).or_default() += weight;
            }
        }
    }

    let seeds = select_taste_seeds(&books, &listening_boost);

    // --- Primary path: storefront candidates not in the owned library ---
    let mut scored: HashMap<String, Recommendation> = HashMap::new();

    if opts.fetch_storefront_candidates && !seeds.is_empty() {
        let fetch_opts = CandidateFetchOptions {
            region: opts.region.clone(),
            seed_limit: opts.storefront_seed_limit,
            max_remote_calls: opts.storefront_max_remote_calls,
            ..CandidateFetchOptions::default()
        };
        let candidates =
            gather_storefront_candidates(library, &seeds, &owned_asins, &owned_isbns, &fetch_opts)
                .await?;

        let seed_centroid =
            seed_embedding_centroid(library, &seed_work_ids, &opts.embedding_model)?;
        let mut embedder = open_candidate_embedder(opts)?;

        for c in candidates {
            let key = candidate_key(&c);
            let mut score = 1.0;
            let mut reasons = vec![c.origin.clone()];

            if let Some(authors) = &c.authors {
                for a in split_tokens(authors) {
                    if let Some(w) = liked_authors.get(&a) {
                        score += w * 0.9;
                        reasons.push(format!("matches liked author ({a})"));
                    }
                }
            }
            if let Some(series) = &c.series {
                let series_l = series.to_lowercase();
                if seeds.iter().any(|s| {
                    s.series
                        .as_deref()
                        .map(|x| x.to_lowercase() == series_l)
                        .unwrap_or(false)
                }) {
                    score += 6.0;
                    reasons.push(format!("same series (“{series}”)"));
                }
            }

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

            scored.insert(
                key,
                Recommendation {
                    work_id: None,
                    title: c.title,
                    authors: c.authors,
                    series: c.series,
                    series_index: None,
                    asin: c.asin.clone(),
                    isbn: c.isbn.clone(),
                    score,
                    reasons,
                    purchase_hints: Vec::new(),
                    from_request: false,
                    request_uuid: None,
                    candidate_source: Some(c.source),
                    candidate_product_id: Some(c.product_id),
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
            },
        );
    }

    let mut recs: Vec<Recommendation> = scored.into_values().collect();
    recs.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Fix Equal capitalization via python later if needed
    recs.truncate(opts.limit);

    if opts.include_purchase_hints {
        for rec in &mut recs {
            // Prefer known store product ids as purchase hints without extra HTTP when possible.
            if let (Some(source), Some(pid)) = (
                rec.candidate_source.as_deref(),
                rec.candidate_product_id.as_deref(),
            ) {
                if source == "audible" {
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
                } else if source == "libro" {
                    rec.purchase_hints.push(PurchaseHint {
                        source: String::from("libro"),
                        product_id: pid.to_string(),
                        title: Some(rec.title.clone()),
                        url: Some(format!("https://libro.fm/audiobooks/{pid}")),
                    });
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

    Ok(recs)
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

fn split_tokens(s: &str) -> Vec<String> {
    s.split([',', ';', '/', '|'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}
