//! Hybrid recommendation engine (heuristics + embeddings + requests).

use std::collections::{HashMap, HashSet};

use bookclerk_library::{AcquireStatus, LibraryStore, RequestStatus, WorkRecord};

use crate::embed::{bytes_to_vector, similar_works};
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
}

impl Default for RecommendOptions {
    fn default() -> Self {
        Self {
            limit: 20,
            embedding_model: String::from(crate::embed::default_embedding_model_id()),
            region: String::from("us"),
            include_purchase_hints: true,
            external_user_id: None,
        }
    }
}

/// One ranked recommendation.
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
    /// True when sourced from an open title request rather than library similarity.
    pub from_request: bool,
    pub request_uuid: Option<String>,
}

/// Build ranked recommendations for the operator (or a specific ABS user).
pub async fn recommend(
    library: &LibraryStore,
    opts: &RecommendOptions,
) -> Result<Vec<Recommendation>> {
    let books = library.list_books(None)?;
    let works = library.list_works()?;
    let works_by_id: HashMap<String, WorkRecord> =
        works.iter().cloned().map(|w| (w.id.clone(), w)).collect();

    let owned_work_ids: HashSet<String> = books
        .iter()
        .filter_map(|b| library.work_id_for_book(&b.uuid).ok().flatten())
        .collect();

    let owned_asins: HashSet<String> = books
        .iter()
        .filter_map(|b| b.asin.clone())
        .map(|s| s.to_ascii_uppercase())
        .collect();
    let owned_isbns: HashSet<String> = books.iter().filter_map(|b| b.isbn.clone()).collect();

    // Seed works: finished, highly rated, or actively listened.
    let mut seed_work_ids: HashSet<String> = HashSet::new();
    let mut liked_authors: HashMap<String, f64> = HashMap::new();
    let mut liked_categories: HashMap<String, f64> = HashMap::new();
    let mut series_owned: HashMap<String, Vec<(String, Option<String>, String)>> = HashMap::new();

    for book in &books {
        let work_id = library.work_id_for_book(&book.uuid)?;
        if let (Some(series), Some(wid)) = (book.series.clone(), work_id.clone()) {
            series_owned.entry(series).or_default().push((
                book.series_index.clone().unwrap_or_default(),
                Some(wid),
                book.title.clone(),
            ));
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
        if book.acquire_status == AcquireStatus::Acquired {
            weight += 0.5;
        }
        if weight <= 0.0 {
            continue;
        }
        if let Some(wid) = work_id {
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
        if let Some(authors) = &row.authors {
            for a in split_tokens(authors) {
                *liked_authors.entry(a).or_default() += weight;
            }
        }
    }

    let mut scored: HashMap<String, (f64, Vec<String>, WorkRecord)> = HashMap::new();

    // Series gaps: next index among owned series that appears elsewhere in works.
    for (series, owned) in &series_owned {
        let owned_indexes: HashSet<String> = owned.iter().map(|(idx, _, _)| idx.clone()).collect();
        for work in works
            .iter()
            .filter(|w| w.series.as_deref() == Some(series.as_str()))
        {
            if owned_work_ids.contains(&work.id) {
                continue;
            }
            let idx = work.series_index.clone().unwrap_or_default();
            if owned_indexes.contains(&idx) {
                continue;
            }
            let entry = scored
                .entry(work.id.clone())
                .or_insert_with(|| (0.0, Vec::new(), work.clone()));
            entry.0 += 8.0;
            entry.1.push(format!("next in series “{series}”"));
        }
    }

    // Author / category overlap for non-owned works.
    for work in &works {
        if owned_work_ids.contains(&work.id) {
            continue;
        }
        let mut bonus = 0.0;
        let mut reasons = Vec::new();
        if let Some(authors) = &work.authors {
            for a in split_tokens(authors) {
                if let Some(w) = liked_authors.get(&a) {
                    bonus += w * 0.8;
                    reasons.push(format!("same author ({a})"));
                }
            }
        }
        let cats = work.categories.as_ref().or(work.subjects.as_ref());
        if let Some(cats) = cats {
            for c in split_tokens(cats) {
                if let Some(w) = liked_categories.get(&c) {
                    bonus += w * 0.4;
                    reasons.push(format!(" overlapping subject ({c})"));
                }
            }
        }
        if bonus > 0.0 {
            let entry = scored
                .entry(work.id.clone())
                .or_insert_with(|| (0.0, Vec::new(), work.clone()));
            entry.0 += bonus;
            for r in reasons {
                if !entry.1.contains(&r) {
                    entry.1.push(r);
                }
            }
        }
    }

    // Embedding similarity from seed works.
    if !seed_work_ids.is_empty() {
        let mut query: Option<Vec<f32>> = None;
        let mut count = 0usize;
        for wid in &seed_work_ids {
            if let Some((_, blob)) =
                library.get_embedding_vector("work", wid, &opts.embedding_model)?
            {
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
            let exclude: Vec<String> = owned_work_ids.iter().cloned().collect();
            let hits = similar_works(
                library,
                &opts.embedding_model,
                &q,
                &exclude,
                opts.limit.saturating_mul(3).max(20),
            )?;
            for hit in hits {
                if let Some(work) = works_by_id.get(&hit.target_id) {
                    let entry = scored
                        .entry(work.id.clone())
                        .or_insert_with(|| (0.0, Vec::new(), work.clone()));
                    entry.0 += f64::from(hit.score) * 10.0;
                    let reason = format!("similar to titles you finish (sim {:.2})", hit.score);
                    if !entry.1.iter().any(|r| r.starts_with("similar to")) {
                        entry.1.push(reason);
                    }
                }
            }
        }
    }

    // Open requests that are not already owned.
    let mut from_requests = Vec::new();
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
        from_requests.push(req);
    }

    let mut recs: Vec<Recommendation> = scored
        .into_values()
        .map(|(score, reasons, work)| Recommendation {
            work_id: Some(work.id),
            title: work.title,
            authors: work.authors,
            series: work.series,
            series_index: work.series_index,
            asin: work.canonical_asin,
            isbn: work.canonical_isbn,
            score,
            reasons,
            purchase_hints: Vec::new(),
            from_request: false,
            request_uuid: None,
        })
        .collect();

    for req in from_requests {
        recs.push(Recommendation {
            work_id: req.work_id,
            title: req.title,
            authors: req.authors,
            series: None,
            series_index: None,
            asin: req.asin,
            isbn: req.isbn,
            score: 12.0,
            reasons: vec![String::from("open title request")],
            purchase_hints: Vec::new(),
            from_request: true,
            request_uuid: Some(req.uuid),
        });
    }

    recs.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    recs.truncate(opts.limit);

    if opts.include_purchase_hints {
        for rec in &mut recs {
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

    Ok(recs)
}

fn split_tokens(s: &str) -> Vec<String> {
    s.split([',', ';', '/', '|'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}
