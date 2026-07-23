//! Download GraphicAudio title materials (plain MP3) into a cache dir.

use std::path::Path;

use libation_source::{PlainAudioPart, PlainFetch};

use crate::client::GraphicAudioClient;
use crate::error::{GraphicAudioError, Result};

/// Fetch one product id into `cache_dir` via `api/links` Hi/Lo URLs.
pub async fn fetch_title_materials(
    client: &GraphicAudioClient,
    product_id: &str,
    cache_dir: &Path,
) -> Result<PlainFetch> {
    std::fs::create_dir_all(cache_dir)?;
    let title_dir = cache_dir.join(product_id);
    std::fs::create_dir_all(&title_dir)?;

    let links = client.links(product_id).await?;
    let url = links.preferred_url().ok_or_else(|| {
        GraphicAudioError::download(format!("no Lo/Hi download URL for product {product_id}"))
    })?;

    let bytes = client.download_bytes(url).await?;
    let ext = extension_from_url(url);
    let path = title_dir.join(format!("audio{ext}"));
    std::fs::write(&path, &bytes)?;

    let cover_path = None; // cover URLs exist on Product; scan stores metadata only for now

    Ok(PlainFetch {
        parts: vec![PlainAudioPart {
            path,
            title: None,
            duration_ms: None,
        }],
        m4b_path: None,
        cover_path,
        chapters: Vec::new(),
    })
}

fn extension_from_url(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".m4b") {
        ".m4b"
    } else if lower.ends_with(".m4a") {
        ".m4a"
    } else if lower.ends_with(".flac") {
        ".flac"
    } else {
        ".mp3"
    }
}
