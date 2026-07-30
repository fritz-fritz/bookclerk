//! Multi-storefront catalog search for Discover typeahead.
//!
//! Queries Audible + Libro.fm locally, plus every registered
//! [`ContentSource::search_catalog`], then merges hits by bibliographic
//! identity (`work_map_key`) — no pricing.

use std::collections::HashMap;

use bookclerk_enrich::{public_http_client, search_catalog_products};
use bookclerk_source::{CatalogSearchOpts, SourceRegistry};
use serde::Deserialize;

use crate::candidates::StorefrontCandidate;
use crate::error::Result;
use crate::identity::{
    identities_match, merge_candidate_metadata, push_edition, work_map_key, StoreEdition,
    WorkIdentity,
};

/// One autocomplete suggestion (possibly spanning multiple storefronts).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CatalogSearchHit {
    pub work_key: String,
    pub title: String,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub series: Option<String>,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    pub store_editions: Vec<StoreEdition>,
    /// Storefronts that matched (deduped source ids).
    pub sources: Vec<String>,
}

/// Search every configured storefront catalog and merge by work identity.
pub async fn catalog_search(
    registry: &SourceRegistry,
    query: &str,
    region: &str,
    limit: usize,
) -> Result<Vec<CatalogSearchHit>> {
    let q = query.trim();
    if q.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let per_store = limit.clamp(8, 24);
    let region = region.trim().to_ascii_lowercase();
    let region = if region.is_empty() {
        String::from("us")
    } else {
        region
    };

    let (audible, libro) = tokio::join!(
        search_audible(q, &region, per_store),
        search_libro(q, per_store),
    );

    let mut by_key: HashMap<String, StorefrontCandidate> = HashMap::new();
    for batch in [audible, libro] {
        match batch {
            Ok(hits) => {
                for hit in hits {
                    upsert_hit(&mut by_key, hit);
                }
            }
            Err(err) => tracing::debug!(error = %err, "catalog search store failed"),
        }
    }

    let search_opts = CatalogSearchOpts {
        query: q.to_string(),
        region: region.clone(),
        limit: per_store,
    };
    for source in registry.all() {
        let id = source.id();
        // Audible/Libro still use enrich / explore above until they implement
        // search_catalog (default empty would just no-op).
        if id.eq_ignore_ascii_case("audible") || id.eq_ignore_ascii_case("libro") {
            continue;
        }
        match source.search_catalog(&search_opts).await {
            Ok(hits) => {
                for hit in hits {
                    upsert_hit(
                        &mut by_key,
                        StorefrontCandidate {
                            source: id.to_string(),
                            product_id: hit.product_id,
                            title: hit.title,
                            authors: hit.authors,
                            narrators: hit.narrators,
                            series: hit.series,
                            series_index: hit.series_index,
                            asin: hit.asin,
                            isbn: hit.isbn,
                            seed_categories: None,
                            origin: String::from("catalog search"),
                            seed_title: None,
                            store_editions: Vec::new(),
                        },
                    );
                }
            }
            Err(err) => tracing::debug!(source = %id, error = %err, "catalog search store failed"),
        }
    }

    let mut out: Vec<CatalogSearchHit> = by_key
        .into_iter()
        .map(|(work_key, c)| {
            let mut sources: Vec<String> =
                c.store_editions.iter().map(|e| e.source.clone()).collect();
            sources.sort();
            sources.dedup();
            CatalogSearchHit {
                work_key,
                title: c.title,
                authors: c.authors,
                narrators: c.narrators,
                series: c.series,
                asin: c.asin,
                isbn: c.isbn,
                store_editions: c.store_editions,
                sources,
            }
        })
        .collect();

    // Prefer multi-store agreement, then title proximity to the query.
    let q_lower = q.to_ascii_lowercase();
    out.sort_by(|a, b| {
        b.sources
            .len()
            .cmp(&a.sources.len())
            .then_with(|| {
                let a_match = a.title.to_ascii_lowercase().starts_with(&q_lower) as u8;
                let b_match = b.title.to_ascii_lowercase().starts_with(&q_lower) as u8;
                b_match.cmp(&a_match)
            })
            .then_with(|| a.title.len().cmp(&b.title.len()))
            .then_with(|| a.title.cmp(&b.title))
    });
    out.truncate(limit);
    Ok(out)
}

fn upsert_hit(map: &mut HashMap<String, StorefrontCandidate>, mut hit: StorefrontCandidate) {
    push_edition(
        &mut hit.store_editions,
        StoreEdition::new(&hit.source, &hit.product_id),
    );
    if let Some(isbn) = hit.isbn.as_mut() {
        let n = bookclerk_enrich::canonicalize_isbn(isbn);
        if !n.is_empty() {
            *isbn = n;
        }
    }

    let match_key = map.iter().find_map(|(key, existing)| {
        if identities_match(
            WorkIdentity::new(
                hit.asin.as_deref(),
                hit.isbn.as_deref(),
                &hit.title,
                hit.authors.as_deref(),
            ),
            WorkIdentity::new(
                existing.asin.as_deref(),
                existing.isbn.as_deref(),
                &existing.title,
                existing.authors.as_deref(),
            ),
        ) {
            Some(key.clone())
        } else {
            None
        }
    });

    if let Some(old_key) = match_key {
        let mut existing = map.remove(&old_key).expect("just found");
        merge_candidate_metadata(&mut existing, &hit);
        let new_key = work_map_key(
            existing.asin.as_deref(),
            existing.isbn.as_deref(),
            &existing.title,
            existing.authors.as_deref(),
            Some(existing.source.as_str()),
            Some(existing.product_id.as_str()),
        );
        map.insert(new_key, existing);
        return;
    }

    let key = work_map_key(
        hit.asin.as_deref(),
        hit.isbn.as_deref(),
        &hit.title,
        hit.authors.as_deref(),
        Some(hit.source.as_str()),
        Some(hit.product_id.as_str()),
    );
    map.insert(key, hit);
}

async fn search_audible(q: &str, region: &str, limit: usize) -> Result<Vec<StorefrontCandidate>> {
    let http = public_http_client()?;
    let products = search_catalog_products(&http, region, q, None, Some(q)).await?;
    Ok(products
        .into_iter()
        .take(limit)
        .filter(|p| !p.asin.trim().is_empty())
        .map(|p| StorefrontCandidate {
            source: String::from("audible"),
            product_id: p.asin.clone(),
            title: p.title.unwrap_or_else(|| p.asin.clone()),
            authors: p.authors,
            narrators: p.narrators,
            series: p.series,
            series_index: p.series_sequence,
            asin: Some(p.asin),
            isbn: None,
            seed_categories: None,
            origin: String::from("catalog search"),
            seed_title: None,
            store_editions: Vec::new(),
        })
        .collect())
}

async fn search_libro(q: &str, limit: usize) -> Result<Vec<StorefrontCandidate>> {
    let http = public_http_client()?;
    let url = format!(
        "https://libro.fm/explore/search?page=1&q={}",
        urlencoding_minimal(q)
    );
    let resp = http.get(&url).send().await?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    let body: LibroExploreSearch = match resp.json().await {
        Ok(b) => b,
        Err(_) => return Ok(Vec::new()),
    };
    let books = body
        .audiobook_collection
        .map(|c| c.audiobooks)
        .unwrap_or_default();
    Ok(books
        .into_iter()
        .take(limit)
        .filter_map(|book| {
            let isbn = book.isbn.filter(|s| !s.is_empty())?;
            let title = book
                .title
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| isbn.clone());
            Some(StorefrontCandidate {
                source: String::from("libro"),
                product_id: isbn.clone(),
                title,
                authors: book.authors.filter(|s| !s.is_empty()),
                narrators: None,
                series: None,
                series_index: None,
                asin: None,
                isbn: Some(isbn),
                seed_categories: None,
                origin: String::from("catalog search"),
                seed_title: None,
                store_editions: Vec::new(),
            })
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct LibroExploreSearch {
    #[serde(default)]
    audiobook_collection: Option<LibroCollection>,
}

#[derive(Debug, Deserialize)]
struct LibroCollection {
    #[serde(default)]
    audiobooks: Vec<LibroBook>,
}

#[derive(Debug, Deserialize)]
struct LibroBook {
    isbn: Option<String>,
    title: Option<String>,
    #[serde(default)]
    authors: Option<String>,
}

fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push('+'),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}
