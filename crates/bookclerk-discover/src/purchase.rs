//! Suggest storefronts where a title might be purchased (with live pricing).

use bookclerk_enrich::{
    normalize_region, public_http_client, region_tld, search_catalog_asins, search_catalog_keywords,
};
use bookclerk_source::{PurchaseHintOpts, SourcePurchaseHint, SourceRegistry};
use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

/// A purchase / catalog availability hint (optionally priced at view time).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PurchaseHint {
    pub source: String,
    pub product_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    /// Lowest known sell price in minor units (cents). `0` = free.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_cents: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Display string from the store (`$2.99`, `FREE`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_label: Option<String>,
}

impl PurchaseHint {
    /// URL-only catalog link (no price yet).
    #[must_use]
    pub fn link(
        source: impl Into<String>,
        product_id: impl Into<String>,
        title: Option<String>,
        url: Option<String>,
    ) -> Self {
        Self {
            source: source.into(),
            product_id: product_id.into(),
            title,
            url,
            price_cents: None,
            currency: None,
            price_label: None,
        }
    }

    fn with_price(mut self, cents: i64, currency: &str, label: impl Into<String>) -> Self {
        self.price_cents = Some(cents.max(0));
        self.currency = Some(currency.to_string());
        self.price_label = Some(label.into());
        self
    }

    fn from_source_hint(source: &str, hint: SourcePurchaseHint) -> Self {
        Self {
            source: source.to_string(),
            product_id: hint.product_id,
            title: hint.title,
            url: hint.url,
            price_cents: hint.price_cents,
            currency: hint.currency,
            price_label: hint.price_label,
        }
    }
}

/// Inputs for view-time catalog + pricing lookup.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PurchaseHintsQuery {
    pub title: String,
    pub authors: Option<String>,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    pub candidate_source: Option<String>,
    pub candidate_product_id: Option<String>,
    /// Known storefront editions already on the recommendation card.
    #[serde(default)]
    pub store_editions: Vec<crate::identity::StoreEdition>,
    pub region: Option<String>,
    /// Storefronts the caller has accounts for — used to pick “best” price
    /// among linked stores while still returning every catalog match.
    #[serde(default)]
    pub preferred_sources: Vec<String>,
}

/// Priced catalog matches for one title, sorted best-first.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PurchaseHintsResponse {
    pub hints: Vec<PurchaseHint>,
    /// Lowest-priced hint (or first catalog hit when no prices resolved).
    pub best: Option<PurchaseHint>,
}

/// Look up Audible (public catalog) and Libro.fm (explore) for a title.
///
/// URL-only for Audible/Libro; registered sources use [`ContentSource::purchase_hint`].
/// Call [`resolve_purchase_hints`] for multi-store + live pricing.
pub async fn purchase_hints_for(
    registry: &SourceRegistry,
    title: &str,
    author: Option<&str>,
    asin: Option<&str>,
    isbn: Option<&str>,
    region: &str,
) -> Result<Vec<PurchaseHint>> {
    let http = public_http_client()?;
    let mut hints = Vec::new();
    let region = normalize_region(region);

    if let Some(asin) = asin.map(str::trim).filter(|s| !s.is_empty()) {
        hints.push(audible_hint(asin, Some(title.to_string()), &region));
    } else if !title.trim().is_empty() {
        let asins = search_catalog_asins(&http, &region, title, author).await?;
        if let Some(asin) = asins.into_iter().next() {
            hints.push(audible_hint(&asin, Some(title.to_string()), &region));
        } else if let Some(isbn) = isbn {
            let asins = search_catalog_keywords(&http, &region, isbn).await?;
            if let Some(asin) = asins.into_iter().next() {
                hints.push(audible_hint(&asin, Some(title.to_string()), &region));
            }
        }
    }

    if let Some(isbn) = isbn.map(str::trim).filter(|s| !s.is_empty()) {
        hints.push(libro_hint(isbn, Some(title.to_string())));
    } else if let Some(hit) = libro_explore_search(&http, title, author).await? {
        hints.push(hit);
    }

    let opts = PurchaseHintOpts {
        product_id: None,
        title: Some(title.to_string()).filter(|s| !s.trim().is_empty()),
        authors: author.map(str::to_string),
        asin: asin.map(str::to_string),
        isbn: isbn.map(str::to_string),
        region: region.clone(),
        with_price: false,
    };
    append_registry_hints(registry, &mut hints, &opts, None).await;

    Ok(hints)
}

/// Seed a deterministic storefront URL from a known candidate (no remote I/O).
#[must_use]
pub fn seed_purchase_hint(
    source: &str,
    product_id: &str,
    title: Option<String>,
    region: &str,
) -> Option<PurchaseHint> {
    let pid = product_id.trim();
    if pid.is_empty() {
        return None;
    }
    let region = normalize_region(region);
    match source.trim().to_ascii_lowercase().as_str() {
        "audible" => Some(audible_hint(pid, title, &region)),
        "libro" => Some(libro_hint(pid, title)),
        "chirp" => Some(PurchaseHint::link(
            "chirp",
            pid,
            title,
            Some(format!("https://www.chirpbooks.com/audiobooks/{pid}")),
        )),
        "graphicaudio" => Some(PurchaseHint::link(
            "graphicaudio",
            pid,
            title,
            Some(format!(
                "https://www.graphicaudio.net/catalog/product/view/id/{pid}"
            )),
        )),
        _ => None,
    }
}

/// Resolve every catalog match and attach live prices (view-time).
pub async fn resolve_purchase_hints(
    registry: &SourceRegistry,
    query: &PurchaseHintsQuery,
) -> Result<PurchaseHintsResponse> {
    let region = normalize_region(query.region.as_deref().unwrap_or("us"));
    let title = query.title.trim();
    let authors = query
        .authors
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let asin = query
        .asin
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let isbn = query
        .isbn
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut hints: Vec<PurchaseHint> = Vec::new();

    for ed in &query.store_editions {
        if let Some(seed) =
            seed_purchase_hint(&ed.source, &ed.product_id, Some(title.to_string()), &region)
        {
            push_dedupe(&mut hints, seed);
        }
    }

    if let (Some(source), Some(pid)) = (
        query.candidate_source.as_deref(),
        query.candidate_product_id.as_deref(),
    ) {
        if let Some(seed) = seed_purchase_hint(source, pid, Some(title.to_string()), &region) {
            push_dedupe(&mut hints, seed);
        }
        // Candidate ASIN/ISBN may differ from product id.
        if source.eq_ignore_ascii_case("audible") {
            // already seeded
        } else if let Some(a) = asin {
            push_dedupe(
                &mut hints,
                audible_hint(a, Some(title.to_string()), &region),
            );
        }
        if source.eq_ignore_ascii_case("libro") {
            // already seeded
        } else if let Some(i) = isbn {
            push_dedupe(&mut hints, libro_hint(i, Some(title.to_string())));
        }
    } else {
        if let Some(a) = asin {
            push_dedupe(
                &mut hints,
                audible_hint(a, Some(title.to_string()), &region),
            );
        }
        if let Some(i) = isbn {
            push_dedupe(&mut hints, libro_hint(i, Some(title.to_string())));
        }
    }

    // Cross-store catalog expansion (Audible + Libro explore + registered sources).
    match purchase_hints_for(registry, title, authors, asin, isbn, &region).await {
        Ok(extra) => {
            for h in extra {
                push_dedupe(&mut hints, h);
            }
        }
        Err(err) => tracing::debug!(error = %err, "purchase catalog expand failed"),
    }

    // Live prices from registered sources (Chirp / GraphicAudio / …).
    let priced_opts = PurchaseHintOpts {
        product_id: query.candidate_product_id.clone(),
        title: Some(title.to_string()).filter(|s| !s.is_empty()),
        authors: authors.map(str::to_string),
        asin: asin.map(str::to_string),
        isbn: isbn.map(str::to_string),
        region: region.clone(),
        with_price: true,
    };
    append_registry_hints(
        registry,
        &mut hints,
        &priced_opts,
        query.candidate_source.as_deref(),
    )
    .await;

    // Also price known editions via registry when we already have a product id.
    for ed in &query.store_editions {
        if ed.source.eq_ignore_ascii_case("audible") || ed.source.eq_ignore_ascii_case("libro") {
            continue;
        }
        let opts = PurchaseHintOpts {
            product_id: Some(ed.product_id.clone()),
            title: Some(title.to_string()).filter(|s| !s.is_empty()),
            authors: authors.map(str::to_string),
            asin: asin.map(str::to_string),
            isbn: isbn.map(str::to_string),
            region: region.clone(),
            with_price: true,
        };
        if let Some(source) = registry.get(&ed.source) {
            match source.purchase_hint(&opts).await {
                Ok(Some(hint)) => {
                    merge_or_push(
                        &mut hints,
                        PurchaseHint::from_source_hint(source.id(), hint),
                    );
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::debug!(source = %ed.source, error = %err, "purchase_hint failed")
                }
            }
        }
    }

    enrich_hints_with_prices(&mut hints, &region).await;
    let preferred = preferred_source_set(&query.preferred_sources);
    sort_hints_for_display(&mut hints, &preferred);
    let best = best_purchase_hint_preferring(&hints, &preferred).cloned();
    Ok(PurchaseHintsResponse { hints, best })
}

/// Pick the lowest-priced hint (ties keep earlier order). Unpriced sort after priced.
#[must_use]
pub fn best_purchase_hint(hints: &[PurchaseHint]) -> Option<&PurchaseHint> {
    hints.iter().min_by(|a, b| cmp_hint_price(a, b))
}

/// Prefer lowest **priced** offer among linked storefronts; otherwise global lowest.
///
/// Unpriced linked stores do not beat a cheaper priced offer elsewhere — we still
/// search every store and only bias the “best” highlight toward accounts the
/// caller can actually use when prices are known.
#[must_use]
pub fn best_purchase_hint_preferring<'a>(
    hints: &'a [PurchaseHint],
    preferred: &std::collections::HashSet<String>,
) -> Option<&'a PurchaseHint> {
    if !preferred.is_empty() {
        let among_linked_priced: Vec<_> = hints
            .iter()
            .filter(|h| {
                preferred.contains(&h.source.to_ascii_lowercase()) && h.price_cents.is_some()
            })
            .collect();
        if let Some(best) = among_linked_priced
            .into_iter()
            .min_by(|a, b| cmp_hint_price(a, b))
        {
            return Some(best);
        }
    }
    best_purchase_hint(hints)
}

/// Resolve many purchase-hint queries with bounded concurrency (order preserved).
pub async fn resolve_purchase_hints_batch(
    registry: &SourceRegistry,
    queries: &[PurchaseHintsQuery],
    max_concurrent: usize,
) -> Vec<Result<PurchaseHintsResponse>> {
    let limit = max_concurrent.clamp(1, 8);
    let mut out = Vec::with_capacity(queries.len());
    for chunk in queries.chunks(limit) {
        let mut set = tokio::task::JoinSet::new();
        for (offset, q) in chunk.iter().enumerate() {
            let q = q.clone();
            let registry = registry.clone();
            set.spawn(async move { (offset, resolve_purchase_hints(&registry, &q).await) });
        }
        let mut slot: Vec<Option<Result<PurchaseHintsResponse>>> =
            (0..chunk.len()).map(|_| None).collect();
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((offset, result)) => slot[offset] = Some(result),
                Err(err) => tracing::debug!(error = %err, "purchase-hints batch task failed"),
            }
        }
        for result in slot {
            out.push(result.unwrap_or_else(|| {
                Err(crate::error::DiscoverError::message(
                    "purchase-hints batch task cancelled",
                ))
            }));
        }
    }
    out
}

async fn append_registry_hints(
    registry: &SourceRegistry,
    hints: &mut Vec<PurchaseHint>,
    opts: &PurchaseHintOpts,
    prefer_source: Option<&str>,
) {
    for source in registry.all() {
        let id = source.id();
        if id.eq_ignore_ascii_case("audible") || id.eq_ignore_ascii_case("libro") {
            continue;
        }
        let mut call_opts = opts.clone();
        // When the candidate is for this source, prefer its product id.
        if prefer_source.is_some_and(|s| s.eq_ignore_ascii_case(id)) {
            // product_id already set from query
        } else if prefer_source.is_some() {
            // Candidate belongs to another store — still search by title.
            call_opts.product_id = None;
        }
        match source.purchase_hint(&call_opts).await {
            Ok(Some(hint)) => {
                merge_or_push(hints, PurchaseHint::from_source_hint(id, hint));
            }
            Ok(None) => {}
            Err(err) => tracing::debug!(source = %id, error = %err, "purchase_hint failed"),
        }
    }
}

fn merge_or_push(hints: &mut Vec<PurchaseHint>, hint: PurchaseHint) {
    let key = (
        hint.source.to_ascii_lowercase(),
        hint.product_id.to_ascii_lowercase(),
    );
    if let Some(existing) = hints.iter_mut().find(|h| {
        h.source.eq_ignore_ascii_case(&key.0) && h.product_id.eq_ignore_ascii_case(&key.1)
    }) {
        if existing.price_cents.is_none() && hint.price_cents.is_some() {
            existing.price_cents = hint.price_cents;
            existing.currency = hint.currency;
            existing.price_label = hint.price_label;
        }
        if existing.url.is_none() {
            existing.url = hint.url;
        }
        if existing.title.is_none() {
            existing.title = hint.title;
        }
        return;
    }
    push_dedupe(hints, hint);
}

fn preferred_source_set(raw: &[String]) -> std::collections::HashSet<String> {
    raw.iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn cmp_hint_price(a: &PurchaseHint, b: &PurchaseHint) -> std::cmp::Ordering {
    match (a.price_cents, b.price_cents) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn sort_hints_for_display(
    hints: &mut [PurchaseHint],
    preferred: &std::collections::HashSet<String>,
) {
    hints.sort_by(|a, b| {
        let a_pref = preferred.contains(&a.source.to_ascii_lowercase());
        let b_pref = preferred.contains(&b.source.to_ascii_lowercase());
        match (a_pref, b_pref) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => cmp_hint_price(a, b),
        }
    });
}

fn push_dedupe(hints: &mut Vec<PurchaseHint>, hint: PurchaseHint) {
    let key = (
        hint.source.to_ascii_lowercase(),
        hint.product_id.to_ascii_lowercase(),
    );
    if hints
        .iter()
        .any(|h| h.source.eq_ignore_ascii_case(&key.0) && h.product_id.eq_ignore_ascii_case(&key.1))
    {
        return;
    }
    // One row per source: keep first (usually the proposing / known id).
    if hints
        .iter()
        .any(|h| h.source.eq_ignore_ascii_case(&hint.source))
    {
        return;
    }
    hints.push(hint);
}

fn audible_hint(asin: &str, title: Option<String>, region: &str) -> PurchaseHint {
    let asin = asin.to_ascii_uppercase();
    PurchaseHint::link(
        "audible",
        asin.clone(),
        title,
        Some(format!(
            "https://www.audible{}/pd/{}",
            region_host_suffix(region),
            asin
        )),
    )
}

fn libro_hint(isbn_or_slug: &str, title: Option<String>) -> PurchaseHint {
    PurchaseHint::link(
        "libro",
        isbn_or_slug,
        title,
        Some(format!("https://libro.fm/audiobooks/{isbn_or_slug}")),
    )
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
    slug: Option<String>,
}

async fn libro_explore_search(
    http: &reqwest::Client,
    title: &str,
    author: Option<&str>,
) -> Result<Option<PurchaseHint>> {
    let q = match author {
        Some(a) if !a.is_empty() => format!("{title} {a}"),
        _ => title.to_string(),
    };
    let url = format!(
        "https://libro.fm/explore/search?page=1&q={}",
        crate_urlencode(&q)
    );
    let resp = http.get(&url).send().await?;
    if !resp.status().is_success() {
        tracing::debug!(status = %resp.status(), "libro explore search non-success");
        return Ok(None);
    }
    let body: LibroExploreSearch = match resp.json().await {
        Ok(b) => b,
        Err(err) => {
            tracing::debug!(error = %err, "libro explore search parse failed");
            return Ok(None);
        }
    };
    let Some(book) = body
        .audiobook_collection
        .and_then(|c| c.audiobooks.into_iter().next())
    else {
        return Ok(None);
    };
    let Some(isbn) = book.isbn.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let url = book
        .slug
        .map(|s| format!("https://libro.fm/audiobooks/{s}"))
        .or_else(|| Some(format!("https://libro.fm/audiobooks/{isbn}")));
    Ok(Some(PurchaseHint::link("libro", isbn, book.title, url)))
}

async fn enrich_hints_with_prices(hints: &mut [PurchaseHint], region: &str) {
    // Sequential soft-fail lookups for Audible/Libro (plugin sources price via purchase_hint).
    for hint in hints.iter_mut() {
        if hint.price_cents.is_some() {
            continue;
        }
        match hint.source.as_str() {
            "audible" => {
                if let Some(priced) = fetch_audible_price(&hint.product_id, region).await {
                    *hint = std::mem::take(hint).with_price(
                        priced.cents,
                        &priced.currency,
                        priced.label,
                    );
                }
            }
            "libro" => {
                if let Some(priced) = fetch_libro_price(&hint.product_id).await {
                    *hint = std::mem::take(hint).with_price(
                        priced.cents,
                        &priced.currency,
                        priced.label,
                    );
                }
            }
            _ => {}
        }
    }
}

struct Priced {
    cents: i64,
    currency: String,
    label: String,
}

async fn fetch_audible_price(asin: &str, region: &str) -> Option<Priced> {
    let http = public_http_client().ok()?;
    let region = normalize_region(region);
    let url = format!(
        "https://api.audible{}/1.0/catalog/products",
        region_tld(&region)
    );
    let resp = http
        .get(&url)
        .query(&[
            ("asins", asin),
            ("num_results", "1"),
            ("response_groups", "price,product_desc,product_attrs"),
        ])
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    let products = body.get("products")?.as_array()?;
    let product = products.iter().find(|p| {
        p.get("asin")
            .and_then(Value::as_str)
            .is_some_and(|a| a.eq_ignore_ascii_case(asin))
    })?;
    parse_audible_price_value(product.get("price")?)
}

/// Parse Audible `price` response-group object.
fn parse_audible_price_value(price: &Value) -> Option<Priced> {
    let lowest = price
        .get("lowest_price")
        .or_else(|| price.get("list_price"))?;
    let amount = lowest.get("base")?.as_f64()?;
    let currency = lowest
        .get("currency_code")
        .and_then(Value::as_str)
        .unwrap_or("USD");
    let cents = (amount * 100.0).round() as i64;
    Some(Priced {
        cents: cents.max(0),
        currency: currency.to_string(),
        label: format_money_label(cents, currency),
    })
}

async fn fetch_libro_price(isbn_or_slug: &str) -> Option<Priced> {
    // Explore details often blocks datacenter IPs; soft-fail without poisoning the link.
    let http = public_http_client().ok()?;
    let url = format!("https://libro.fm/explore/audiobook_details/{isbn_or_slug}");
    let resp = http.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    find_price_in_json(&body)
}

fn find_price_in_json(value: &Value) -> Option<Priced> {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key = k.to_ascii_lowercase();
                if matches!(
                    key.as_str(),
                    "price" | "sale_price" | "current_price" | "list_price" | "amount"
                ) {
                    if let Some(p) = price_from_json_node(v) {
                        return Some(p);
                    }
                }
            }
            for v in map.values() {
                if let Some(p) = find_price_in_json(v) {
                    return Some(p);
                }
            }
            None
        }
        Value::Array(items) => {
            for v in items {
                if let Some(p) = find_price_in_json(v) {
                    return Some(p);
                }
            }
            None
        }
        _ => None,
    }
}

fn price_from_json_node(v: &Value) -> Option<Priced> {
    if let Some(s) = v.as_str() {
        if let Some(cents) = parse_money_label_to_cents(s) {
            return Some(Priced {
                cents,
                currency: String::from("USD"),
                label: s.trim().to_string(),
            });
        }
    }
    if let Some(n) = v.as_f64() {
        let cents = if n >= 1000.0 {
            // Heuristic: large ints are often cents already.
            n.round() as i64
        } else {
            (n * 100.0).round() as i64
        };
        return Some(Priced {
            cents: cents.max(0),
            currency: String::from("USD"),
            label: format_money_label(cents, "USD"),
        });
    }
    if let Some(obj) = v.as_object() {
        if let Some(amount) = obj
            .get("amount")
            .or_else(|| obj.get("value"))
            .and_then(Value::as_f64)
        {
            let currency = obj
                .get("currency")
                .or_else(|| obj.get("currency_code"))
                .and_then(Value::as_str)
                .unwrap_or("USD");
            let cents = (amount * 100.0).round() as i64;
            return Some(Priced {
                cents: cents.max(0),
                currency: currency.to_string(),
                label: format_money_label(cents, currency),
            });
        }
    }
    None
}

/// Parse `$12.34` / `12.34` / `FREE` into cents.
#[must_use]
pub fn parse_money_label_to_cents(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if s.eq_ignore_ascii_case("free") || s.eq_ignore_ascii_case("free!") {
        return Some(0);
    }
    let mut num = String::new();
    let mut seen_dot = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else if c == '.' && !seen_dot {
            num.push('.');
            seen_dot = true;
        } else if c == ',' {
            continue;
        } else if !num.is_empty() {
            break;
        }
    }
    if num.is_empty() {
        return None;
    }
    let amount: f64 = num.parse().ok()?;
    Some((amount * 100.0).round() as i64)
}

#[must_use]
pub fn format_money_label(cents: i64, currency: &str) -> String {
    if cents <= 0 {
        return String::from("FREE");
    }
    let major = cents / 100;
    let minor = (cents % 100).unsigned_abs();
    match currency.to_ascii_uppercase().as_str() {
        "USD" | "" => format!("${major}.{minor:02}"),
        "GBP" => format!("£{major}.{minor:02}"),
        "EUR" => format!("€{major}.{minor:02}"),
        other => format!("{major}.{minor:02} {other}"),
    }
}

fn crate_urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn money_label_parsing() {
        assert_eq!(parse_money_label_to_cents("$2.99"), Some(299));
        assert_eq!(parse_money_label_to_cents("FREE"), Some(0));
        assert_eq!(parse_money_label_to_cents("12.5"), Some(1250));
        assert_eq!(format_money_label(299, "USD"), "$2.99");
        assert_eq!(format_money_label(0, "USD"), "FREE");
    }

    #[test]
    fn audible_price_json() {
        let price = json!({
            "credit_price": 1.0,
            "list_price": {
                "base": 25.219999313354492,
                "currency_code": "USD",
                "type": "list"
            },
            "lowest_price": {
                "base": 14.95,
                "currency_code": "USD",
                "type": "member"
            }
        });
        let priced = parse_audible_price_value(&price).unwrap();
        assert_eq!(priced.cents, 1495);
        assert_eq!(priced.label, "$14.95");
    }

    #[test]
    fn best_hint_prefers_lowest_price() {
        let hints = vec![
            PurchaseHint::link("audible", "A", None, None).with_price(1999, "USD", "$19.99"),
            PurchaseHint::link("chirp", "C", None, None).with_price(299, "USD", "$2.99"),
            PurchaseHint::link("libro", "L", None, None),
        ];
        let best = best_purchase_hint(&hints).unwrap();
        assert_eq!(best.source, "chirp");
        assert_eq!(best.price_cents, Some(299));
    }

    #[test]
    fn best_hint_prefers_priced_linked_store() {
        let hints = vec![
            PurchaseHint::link("audible", "A", None, None).with_price(1999, "USD", "$19.99"),
            PurchaseHint::link("chirp", "C", None, None).with_price(299, "USD", "$2.99"),
            PurchaseHint::link("libro", "L", None, None).with_price(999, "USD", "$9.99"),
        ];
        let preferred = std::collections::HashSet::from([String::from("audible")]);
        let best = best_purchase_hint_preferring(&hints, &preferred).unwrap();
        assert_eq!(best.source, "audible");
    }

    #[test]
    fn unpriced_linked_does_not_beat_cheaper_foreign() {
        let hints = vec![
            PurchaseHint::link("audible", "A", None, None),
            PurchaseHint::link("chirp", "C", None, None).with_price(299, "USD", "$2.99"),
        ];
        let preferred = std::collections::HashSet::from([String::from("audible")]);
        let best = best_purchase_hint_preferring(&hints, &preferred).unwrap();
        assert_eq!(best.source, "chirp");
    }
}
