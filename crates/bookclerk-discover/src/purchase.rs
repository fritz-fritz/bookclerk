//! Suggest storefronts where a title might be purchased.

use bookclerk_enrich::{
    normalize_region, public_http_client, search_catalog_asins, search_catalog_keywords,
};
use serde::Deserialize;

use crate::error::Result;

/// A purchase / catalog availability hint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PurchaseHint {
    pub source: String,
    pub product_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
}

/// Look up Audible (public catalog) and Libro.fm (explore) for a title.
pub async fn purchase_hints_for(
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
        hints.push(PurchaseHint {
            source: String::from("audible"),
            product_id: asin.to_ascii_uppercase(),
            title: Some(title.to_string()),
            url: Some(format!(
                "https://www.audible{}/pd/{}",
                region_host_suffix(&region),
                asin.to_ascii_uppercase()
            )),
        });
    } else if !title.trim().is_empty() {
        let asins = search_catalog_asins(&http, &region, title, author).await?;
        if let Some(asin) = asins.into_iter().next() {
            hints.push(PurchaseHint {
                source: String::from("audible"),
                product_id: asin.clone(),
                title: Some(title.to_string()),
                url: Some(format!(
                    "https://www.audible{}/pd/{}",
                    region_host_suffix(&region),
                    asin
                )),
            });
        } else if let Some(isbn) = isbn {
            let asins = search_catalog_keywords(&http, &region, isbn).await?;
            if let Some(asin) = asins.into_iter().next() {
                hints.push(PurchaseHint {
                    source: String::from("audible"),
                    product_id: asin.clone(),
                    title: Some(title.to_string()),
                    url: Some(format!(
                        "https://www.audible{}/pd/{}",
                        region_host_suffix(&region),
                        asin
                    )),
                });
            }
        }
    }

    if let Some(isbn) = isbn.map(str::trim).filter(|s| !s.is_empty()) {
        hints.push(PurchaseHint {
            source: String::from("libro"),
            product_id: isbn.to_string(),
            title: Some(title.to_string()),
            url: Some(format!("https://libro.fm/audiobooks/{isbn}")),
        });
    } else if let Some(hit) = libro_explore_search(&http, title, author).await? {
        hints.push(hit);
    }

    Ok(hints)
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
        // Explore path may require Accept headers or change shape; soft-fail.
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
    Ok(Some(PurchaseHint {
        source: String::from("libro"),
        product_id: isbn,
        title: book.title,
        url,
    }))
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
