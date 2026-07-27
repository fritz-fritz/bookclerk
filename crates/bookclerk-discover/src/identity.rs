//! Canonical identity for discovery candidates / recommendations.
//!
//! Prefer bibliographic ids (ISBN, then ASIN), then soft title+author matching
//! so the same book from multiple storefronts collapses to one card.

use bookclerk_enrich::{
    clean_author_for_compares, clean_title_for_compares, levenshtein_similarity, normalize_isbn,
};

use crate::candidates::StorefrontCandidate;
use crate::recommend::Recommendation;

/// One storefront edition of a work (for multi-store purchase links).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
pub struct StoreEdition {
    pub source: String,
    pub product_id: String,
}

impl StoreEdition {
    #[must_use]
    pub fn new(source: impl Into<String>, product_id: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            product_id: product_id.into(),
        }
    }
}

/// Hard bibliographic key: normalized ISBN first, then ASIN.
#[must_use]
pub fn hard_work_key(asin: Option<&str>, isbn: Option<&str>) -> Option<String> {
    if let Some(isbn) = isbn.map(normalize_isbn).filter(|s| !s.is_empty()) {
        return Some(format!("isbn:{isbn}"));
    }
    let asin = asin.map(str::trim).filter(|s| !s.is_empty())?;
    Some(format!("asin:{}", asin.to_ascii_uppercase()))
}

/// Soft key from cleaned title + primary author (exact string match only).
#[must_use]
pub fn soft_work_key(title: &str, authors: Option<&str>) -> String {
    let t = clean_title_for_compares(title, false);
    let a = primary_author_cleaned(authors).unwrap_or_default();
    format!("soft:{t}|{a}")
}

/// Best stable map key for a recommendation / candidate.
#[must_use]
pub fn work_map_key(
    asin: Option<&str>,
    isbn: Option<&str>,
    title: &str,
    authors: Option<&str>,
    source: Option<&str>,
    product_id: Option<&str>,
) -> String {
    if let Some(k) = hard_work_key(asin, isbn) {
        return k;
    }
    if !title.trim().is_empty() {
        let soft = soft_work_key(title, authors);
        // Avoid collapsing unrelated empties into one bucket.
        if soft != "soft:|" {
            return soft;
        }
    }
    match (source, product_id) {
        (Some(s), Some(p)) if !s.is_empty() && !p.is_empty() => format!("{s}:{p}"),
        (_, Some(p)) if !p.is_empty() => format!("product:{p}"),
        _ => format!("title:{}", title.trim().to_ascii_lowercase()),
    }
}

#[must_use]
pub fn candidate_map_key(c: &StorefrontCandidate) -> String {
    work_map_key(
        c.asin.as_deref(),
        c.isbn.as_deref(),
        &c.title,
        c.authors.as_deref(),
        Some(c.source.as_str()),
        Some(c.product_id.as_str()),
    )
}

#[must_use]
pub fn recommendation_map_key(r: &Recommendation) -> String {
    work_map_key(
        r.asin.as_deref(),
        r.isbn.as_deref(),
        &r.title,
        r.authors.as_deref(),
        r.candidate_source.as_deref(),
        r.candidate_product_id.as_deref(),
    )
}

/// Whether two titles should be treated as the same work.
#[must_use]
pub fn works_match(
    title_a: &str,
    authors_a: Option<&str>,
    title_b: &str,
    authors_b: Option<&str>,
) -> bool {
    let ta = clean_title_for_compares(title_a, false);
    let tb = clean_title_for_compares(title_b, false);
    if ta.is_empty() || tb.is_empty() {
        return false;
    }
    let title_sim = levenshtein_similarity(&ta, &tb);
    if title_sim < 0.90 {
        return false;
    }
    match (
        primary_author_cleaned(authors_a),
        primary_author_cleaned(authors_b),
    ) {
        (Some(a), Some(b)) => {
            if a == b {
                return title_sim >= 0.90;
            }
            levenshtein_similarity(&a, &b) >= 0.85 && title_sim >= 0.92
        }
        // One side missing author: require a tighter title match.
        (None, None) => title_sim >= 0.96,
        _ => title_sim >= 0.95,
    }
}

fn primary_author_cleaned(authors: Option<&str>) -> Option<String> {
    let raw = authors?
        .split([',', ';', '&', '/'])
        .map(str::trim)
        .find(|s| !s.is_empty())?;
    let cleaned = clean_author_for_compares(raw);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Merge storefront metadata into `into`, preferring filled fields / richer text.
pub fn merge_candidate_metadata(into: &mut StorefrontCandidate, from: &StorefrontCandidate) {
    push_edition(
        &mut into.store_editions,
        StoreEdition::new(&from.source, &from.product_id),
    );
    if into.asin.is_none() {
        into.asin = from.asin.clone();
    }
    if into.isbn.is_none() {
        into.isbn = from.isbn.clone().map(|i| {
            let n = normalize_isbn(&i);
            if n.is_empty() {
                i
            } else {
                n
            }
        });
    } else if let Some(isbn) = into.isbn.as_mut() {
        let n = normalize_isbn(isbn);
        if !n.is_empty() {
            *isbn = n;
        }
    }
    fill_opt_string(&mut into.authors, from.authors.as_deref());
    fill_opt_string(&mut into.narrators, from.narrators.as_deref());
    fill_opt_string(&mut into.series, from.series.as_deref());
    fill_opt_string(&mut into.series_index, from.series_index.as_deref());
    if from.title.len() > into.title.len() && !from.title.is_empty() {
        // Prefer the longer title when both exist (often includes subtitle).
        into.title = from.title.clone();
    }
    if !from.origin.is_empty() && !into.origin.contains(&from.origin) {
        into.origin = format!("{}; {}", into.origin, from.origin);
    }
}

/// Merge a scored recommendation into an existing card.
pub fn merge_recommendation(into: &mut Recommendation, mut from: Recommendation) {
    for ed in std::mem::take(&mut from.store_editions) {
        push_edition(&mut into.store_editions, ed);
    }
    if let (Some(s), Some(p)) = (
        from.candidate_source.as_deref(),
        from.candidate_product_id.as_deref(),
    ) {
        push_edition(&mut into.store_editions, StoreEdition::new(s, p));
    }
    if let (Some(s), Some(p)) = (
        into.candidate_source.as_deref(),
        into.candidate_product_id.as_deref(),
    ) {
        push_edition(&mut into.store_editions, StoreEdition::new(s, p));
    }

    if from.score > into.score {
        into.score = from.score;
        // Keep the higher-scoring store as the primary candidate identity.
        if from.candidate_source.is_some() {
            into.candidate_source = from.candidate_source.clone();
            into.candidate_product_id = from.candidate_product_id.clone();
        }
        if from.title.len() >= into.title.len() {
            into.title = from.title.clone();
        }
    } else {
        // Multi-store agreement is itself a signal.
        into.score += 0.35;
    }

    if into.asin.is_none() {
        into.asin = from.asin.clone();
    }
    if into.isbn.is_none() {
        into.isbn = from.isbn.clone().map(|i| {
            let n = normalize_isbn(&i);
            if n.is_empty() {
                i
            } else {
                n
            }
        });
    }
    fill_opt_string(&mut into.authors, from.authors.as_deref());
    fill_opt_string(&mut into.narrators, from.narrators.as_deref());
    fill_opt_string(&mut into.series, from.series.as_deref());
    fill_opt_string(&mut into.series_index, from.series_index.as_deref());
    fill_opt_string(&mut into.seed_categories, from.seed_categories.as_deref());
    if into.work_id.is_none() {
        into.work_id = from.work_id.clone();
    }
    if from.from_request {
        into.from_request = true;
        if into.request_uuid.is_none() {
            into.request_uuid = from.request_uuid.clone();
        }
    }
    for reason in from.reasons {
        if !into.reasons.iter().any(|r| r == &reason) {
            into.reasons.push(reason);
        }
    }
    for hint in from.purchase_hints {
        if !into.purchase_hints.iter().any(|h| {
            h.source.eq_ignore_ascii_case(&hint.source)
                && h.product_id.eq_ignore_ascii_case(&hint.product_id)
        }) {
            into.purchase_hints.push(hint);
        }
    }
}

pub fn push_edition(editions: &mut Vec<StoreEdition>, edition: StoreEdition) {
    if edition.source.trim().is_empty() || edition.product_id.trim().is_empty() {
        return;
    }
    if editions.iter().any(|e| {
        e.source.eq_ignore_ascii_case(&edition.source)
            && e.product_id.eq_ignore_ascii_case(&edition.product_id)
    }) {
        return;
    }
    // Prefer one edition per source (keep first / primary).
    if editions
        .iter()
        .any(|e| e.source.eq_ignore_ascii_case(&edition.source))
    {
        return;
    }
    editions.push(edition);
}

fn fill_opt_string(slot: &mut Option<String>, incoming: Option<&str>) {
    let Some(v) = incoming.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    match slot {
        None => *slot = Some(v.to_string()),
        Some(existing) if existing.len() < v.len() => *slot = Some(v.to_string()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isbn_preferred_over_asin() {
        let key = hard_work_key(Some("B00TEST"), Some("978-1-234-56789-0")).unwrap();
        assert_eq!(key, "isbn:9781234567890");
    }

    #[test]
    fn soft_match_same_title_author() {
        assert!(works_match(
            "Project Hail Mary",
            Some("Andy Weir"),
            "Project Hail Mary: A Novel",
            Some("Andy Weir")
        ));
    }

    #[test]
    fn soft_match_rejects_different_author() {
        assert!(!works_match(
            "Project Hail Mary",
            Some("Andy Weir"),
            "Project Hail Mary",
            Some("Someone Else")
        ));
    }

    #[test]
    fn merge_keeps_editions_per_source() {
        let mut editions = vec![StoreEdition::new("audible", "B00A")];
        push_edition(&mut editions, StoreEdition::new("libro", "9781"));
        push_edition(&mut editions, StoreEdition::new("audible", "B00B"));
        assert_eq!(editions.len(), 2);
        assert_eq!(editions[1].source, "libro");
    }
}
