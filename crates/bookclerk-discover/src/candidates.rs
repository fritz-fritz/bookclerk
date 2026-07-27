//! Storefront candidate discovery (titles not yet owned).
//!
//! Seeds from local taste (finished / rated / listening), then expands via:
//! - Libro.fm `explore/audiobook_details` → `related_audiobooks`
//! - Audible public catalog author / series keyword search
//!
//! Local embeddings and ownership filters evaluate those remote hits.

use std::collections::{HashMap, HashSet};

use bookclerk_enrich::{public_http_client, search_catalog_products};
use bookclerk_library::{BookRecord, LibraryStore};
use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

/// A purchase candidate discovered from a storefront catalog (not owned locally).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StorefrontCandidate {
    pub source: String,
    pub product_id: String,
    pub title: String,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub series: Option<String>,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    /// How this candidate was found (related-to seed, author search, …).
    pub origin: String,
    pub seed_title: Option<String>,
}

/// Options for storefront candidate expansion.
#[derive(Debug, Clone)]
pub struct CandidateFetchOptions {
    pub region: String,
    /// Max local seed titles to expand from (finished / rated first).
    pub seed_limit: usize,
    /// Cap remote HTTP calls (Libro details + Audible searches).
    pub max_remote_calls: usize,
    pub include_libro_related: bool,
    pub include_audible_author_search: bool,
    pub include_audible_series_search: bool,
}

impl Default for CandidateFetchOptions {
    fn default() -> Self {
        Self {
            region: String::from("us"),
            seed_limit: 8,
            max_remote_calls: 24,
            include_libro_related: true,
            include_audible_author_search: true,
            include_audible_series_search: true,
        }
    }
}

/// Expand storefront catalogs from local taste seeds; drop already-owned ids.
pub async fn gather_storefront_candidates(
    _library: &LibraryStore,
    seeds: &[BookRecord],
    owned_asins: &HashSet<String>,
    owned_isbns: &HashSet<String>,
    opts: &CandidateFetchOptions,
) -> Result<Vec<StorefrontCandidate>> {
    let http = public_http_client()?;
    let mut by_key: HashMap<String, StorefrontCandidate> = HashMap::new();
    let mut remote_calls = 0usize;

    let seeds: Vec<&BookRecord> = seeds.iter().take(opts.seed_limit).collect();

    for seed in &seeds {
        if remote_calls >= opts.max_remote_calls {
            break;
        }

        // Libro related titles (best signal when we have an ISBN).
        if opts.include_libro_related {
            if let Some(isbn) = seed
                .isbn
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if remote_calls < opts.max_remote_calls {
                    match libro_related(&http, isbn).await {
                        Ok(related) => {
                            remote_calls += 1;
                            for mut c in related {
                                c.seed_title = Some(seed.title.clone());
                                c.origin = format!("libro related to “{}”", seed.title);
                                insert_candidate(&mut by_key, c, owned_asins, owned_isbns);
                            }
                        }
                        Err(err) => {
                            remote_calls += 1;
                            tracing::debug!(isbn, error = %err, "libro related lookup failed");
                        }
                    }
                }
            }
        }

        // Audible: more by same author.
        if opts.include_audible_author_search {
            if let Some(author) = primary_author(seed.authors.as_deref()) {
                if remote_calls < opts.max_remote_calls {
                    match search_catalog_products(&http, &opts.region, "", Some(author), None).await
                    {
                        Ok(products) => {
                            remote_calls += 1;
                            for p in products {
                                let c = StorefrontCandidate {
                                    source: String::from("audible"),
                                    product_id: p.asin.clone(),
                                    title: p.title.unwrap_or_else(|| p.asin.clone()),
                                    authors: p.authors.or_else(|| Some(author.to_string())),
                                    narrators: p.narrators,
                                    series: p.series,
                                    asin: Some(p.asin),
                                    isbn: None,
                                    origin: format!("audible author search ({author})"),
                                    seed_title: Some(seed.title.clone()),
                                };
                                insert_candidate(&mut by_key, c, owned_asins, owned_isbns);
                            }
                        }
                        Err(err) => {
                            remote_calls += 1;
                            tracing::debug!(author, error = %err, "audible author search failed");
                        }
                    }
                }
            }
        }

        // Audible: series keyword expansion.
        if opts.include_audible_series_search {
            if let Some(series) = seed
                .series
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if remote_calls < opts.max_remote_calls {
                    match search_catalog_products(&http, &opts.region, "", None, Some(series)).await
                    {
                        Ok(products) => {
                            remote_calls += 1;
                            for p in products {
                                let c = StorefrontCandidate {
                                    source: String::from("audible"),
                                    product_id: p.asin.clone(),
                                    title: p.title.unwrap_or_else(|| p.asin.clone()),
                                    authors: p.authors.clone().or_else(|| seed.authors.clone()),
                                    narrators: p.narrators,
                                    series: p.series.or_else(|| Some(series.to_string())),
                                    asin: Some(p.asin),
                                    isbn: None,
                                    origin: format!("audible series search (“{series}”)"),
                                    seed_title: Some(seed.title.clone()),
                                };
                                insert_candidate(&mut by_key, c, owned_asins, owned_isbns);
                            }
                        }
                        Err(err) => {
                            remote_calls += 1;
                            tracing::debug!(series, error = %err, "audible series search failed");
                        }
                    }
                }
            }
        }
    }

    tracing::info!(
        seeds = seeds.len(),
        remote_calls,
        candidates = by_key.len(),
        "gathered storefront recommendation candidates"
    );
    Ok(by_key.into_values().collect())
}

fn insert_candidate(
    map: &mut HashMap<String, StorefrontCandidate>,
    c: StorefrontCandidate,
    owned_asins: &HashSet<String>,
    owned_isbns: &HashSet<String>,
) {
    if let Some(asin) = c.asin.as_deref() {
        if owned_asins.contains(&asin.to_ascii_uppercase()) {
            return;
        }
    }
    if let Some(isbn) = c.isbn.as_deref() {
        if owned_isbns.contains(isbn) {
            return;
        }
    }
    // Also skip if product_id matches an owned ASIN/ISBN.
    if owned_asins.contains(&c.product_id.to_ascii_uppercase())
        || owned_isbns.contains(&c.product_id)
    {
        return;
    }
    let key = c
        .asin
        .as_deref()
        .map(|a| format!("asin:{}", a.to_ascii_uppercase()))
        .or_else(|| c.isbn.as_deref().map(|i| format!("isbn:{i}")))
        .unwrap_or_else(|| format!("{}:{}", c.source, c.product_id));
    map.entry(key).or_insert(c);
}

fn primary_author(authors: Option<&str>) -> Option<&str> {
    authors?
        .split([',', ';', '&'])
        .map(str::trim)
        .find(|s| !s.is_empty())
}

#[derive(Debug, Deserialize)]
struct LibroDetailsResponse {
    #[serde(default)]
    data: Option<LibroDetailsData>,
}

#[derive(Debug, Deserialize)]
struct LibroDetailsData {
    #[serde(default)]
    related_audiobooks: Vec<Value>,
}

async fn libro_related(http: &reqwest::Client, isbn: &str) -> Result<Vec<StorefrontCandidate>> {
    let url = format!("https://libro.fm/explore/audiobook_details/{isbn}");
    let resp = http.get(&url).send().await?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    let body: LibroDetailsResponse = resp
        .json()
        .await
        .map_err(|e| crate::error::DiscoverError::message(format!("libro related parse: {e}")))?;
    let related = body.data.map(|d| d.related_audiobooks).unwrap_or_default();
    Ok(related.iter().filter_map(parse_libro_book).collect())
}

fn parse_libro_book(v: &Value) -> Option<StorefrontCandidate> {
    let isbn = v
        .get("isbn")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_string();
    let title = v
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_string();
    let authors = v
        .get("authors")
        .and_then(|a| {
            if let Some(s) = a.as_str() {
                Some(s.to_string())
            } else if let Some(arr) = a.as_array() {
                let names: Vec<&str> = arr
                    .iter()
                    .filter_map(|x| x.as_str().or_else(|| x.get("name").and_then(Value::as_str)))
                    .collect();
                if names.is_empty() {
                    None
                } else {
                    Some(names.join(", "))
                }
            } else {
                None
            }
        })
        .or_else(|| v.get("author").and_then(Value::as_str).map(str::to_string));
    let narrators = v
        .get("narrators")
        .and_then(Value::as_str)
        .map(str::to_string);
    let series = v.get("series").and_then(|s| {
        s.as_str()
            .map(str::to_string)
            .or_else(|| s.get("name").and_then(Value::as_str).map(str::to_string))
    });
    Some(StorefrontCandidate {
        source: String::from("libro"),
        product_id: isbn.clone(),
        title,
        authors,
        narrators,
        series,
        asin: None,
        isbn: Some(isbn),
        origin: String::from("libro related"),
        seed_title: None,
    })
}

/// Pick local seed books for storefront expansion (finished / high-rated first).
#[must_use]
pub fn select_taste_seeds(
    books: &[BookRecord],
    listening_boost_uuids: &HashSet<String>,
) -> Vec<BookRecord> {
    let mut scored: Vec<(i32, &BookRecord)> = books
        .iter()
        .map(|b| {
            let mut s = 0;
            if b.is_finished {
                s += 50;
            }
            if listening_boost_uuids.contains(&b.uuid) {
                s += 40;
            }
            if let Some(r) = b.rating_overall {
                if r >= 4.0 {
                    s += 30;
                } else if r >= 3.0 {
                    s += 10;
                }
            }
            if b.isbn.is_some() {
                s += 5;
            }
            if b.asin.is_some() {
                s += 3;
            }
            (s, b)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by_key(|b| std::cmp::Reverse(b.0));
    scored.into_iter().map(|(_, b)| b.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_libro_related_object() {
        let v = json!({
            "isbn": "9781234567890",
            "title": "Next Title",
            "authors": [{"name": "Ada Author"}],
            "series": {"name": "Test Series"}
        });
        let c = parse_libro_book(&v).unwrap();
        assert_eq!(c.isbn.as_deref(), Some("9781234567890"));
        assert_eq!(c.authors.as_deref(), Some("Ada Author"));
        assert_eq!(c.series.as_deref(), Some("Test Series"));
    }
}
