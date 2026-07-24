//! Download GraphicAudio title materials into a cache dir.
//!
//! Access priority (when Magento credentials are available):
//! 1. Magento ZIP (M4B/MP3/FLAC) — preferred; no Access App device slot
//! 2. Browser Player media URL — Magento session + CloudFront cookies
//! 3. Access App `api/links` Hi/Lo — uses device activation token
//!
//! Override with `LIBATION_GA_FETCH=auto|zip|browser|app` (default `auto`).

use std::path::{Path, PathBuf};

use libation_source::{PlainAudioPart, PlainFetch};

use crate::client::{GraphicAudioClient, Product};
use crate::error::{GraphicAudioError, Result};
use crate::magento::{self, MagentoClient};

/// Env override for which GraphicAudio access path to use.
pub const GA_FETCH_ENV: &str = "LIBATION_GA_FETCH";

/// Password env (same as CLI login) for Magento ZIP / Browser Player.
pub const GA_PASSWORD_ENV: &str = "LIBATION_GA_PASSWORD";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaFetchMode {
    Auto,
    Zip,
    Browser,
    App,
}

impl GaFetchMode {
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var(GA_FETCH_ENV)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "zip" | "magento" | "m4b" => Self::Zip,
            "browser" | "player" | "library" => Self::Browser,
            "app" | "access" | "api" => Self::App,
            _ => Self::Auto,
        }
    }
}

/// Read Magento password from [`GA_PASSWORD_ENV`] when set.
#[must_use]
pub fn password_from_env() -> Option<String> {
    std::env::var(GA_PASSWORD_ENV)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Fetch one product id into `cache_dir` via `api/links` Hi/Lo URLs.
pub async fn fetch_title_materials(
    client: &GraphicAudioClient,
    product_id: &str,
    cache_dir: &Path,
) -> Result<PlainFetch> {
    fetch_title_materials_with_quality(client, product_id, cache_dir, true).await
}

/// Like [`fetch_title_materials`], but selects Hi vs Lo from ingest quality.
pub async fn fetch_title_materials_with_quality(
    client: &GraphicAudioClient,
    product_id: &str,
    cache_dir: &Path,
    prefer_hi: bool,
) -> Result<PlainFetch> {
    fetch_access_app(client, product_id, cache_dir, prefer_hi).await
}

/// Parameters for [`fetch_title_best_effort`].
#[derive(Debug, Clone)]
pub struct TitleFetchRequest<'a> {
    pub store_base_url: &'a str,
    pub email: &'a str,
    pub product_id: &'a str,
    pub product_title: Option<&'a str>,
    pub cache_dir: &'a Path,
    pub prefer_hi: bool,
    pub mode: GaFetchMode,
    pub password: Option<&'a str>,
}

/// Full liberate fetch: Magento ZIP → Browser Player → Access App.
pub async fn fetch_title_best_effort(
    access: &GraphicAudioClient,
    req: TitleFetchRequest<'_>,
) -> Result<PlainFetch> {
    let title_dir = req.cache_dir.join(req.product_id);
    std::fs::create_dir_all(&title_dir)?;

    let try_zip = matches!(req.mode, GaFetchMode::Auto | GaFetchMode::Zip);
    let try_browser = matches!(req.mode, GaFetchMode::Auto | GaFetchMode::Browser);
    let try_app = matches!(req.mode, GaFetchMode::Auto | GaFetchMode::App);

    if matches!(req.mode, GaFetchMode::Zip | GaFetchMode::Browser) && req.password.is_none() {
        return Err(GraphicAudioError::auth(format!(
            "{GA_FETCH_ENV} requires {GA_PASSWORD_ENV} for Magento storefront access"
        )));
    }

    if try_zip {
        if let Some(password) = req.password {
            match fetch_magento_zip(
                req.store_base_url,
                req.email,
                password,
                req.product_id,
                req.product_title,
                &title_dir,
            )
            .await
            {
                Ok(plain) => return Ok(plain),
                Err(err) => {
                    if matches!(req.mode, GaFetchMode::Zip) {
                        return Err(err);
                    }
                    tracing::info!(
                        product_id = req.product_id,
                        error = %err,
                        "GraphicAudio Magento ZIP unavailable; trying next access path"
                    );
                }
            }
        } else if matches!(req.mode, GaFetchMode::Auto) {
            tracing::debug!("{GA_PASSWORD_ENV} unset; skipping Magento ZIP / Browser Player");
        }
    }

    if try_browser {
        if let Some(password) = req.password {
            match fetch_browser(
                req.store_base_url,
                req.email,
                password,
                req.product_id,
                &title_dir,
            )
            .await
            {
                Ok(plain) => return Ok(plain),
                Err(err) => {
                    if matches!(req.mode, GaFetchMode::Browser) {
                        return Err(err);
                    }
                    tracing::info!(
                        product_id = req.product_id,
                        error = %err,
                        "GraphicAudio Browser Player unavailable; trying Access App"
                    );
                }
            }
        }
    }

    if try_app {
        return fetch_access_app(access, req.product_id, req.cache_dir, req.prefer_hi).await;
    }

    Err(GraphicAudioError::download(format!(
        "no GraphicAudio access path succeeded for product {}",
        req.product_id
    )))
}

async fn fetch_magento_zip(
    store_base_url: &str,
    email: &str,
    password: &str,
    product_id: &str,
    product_title: Option<&str>,
    title_dir: &Path,
) -> Result<PlainFetch> {
    let title = product_title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            GraphicAudioError::download(format!(
                "Magento ZIP needs a product title to match downloadable rows (product {product_id})"
            ))
        })?;

    let client = MagentoClient::new(store_base_url)?;
    client.login(email, password).await?;
    let audio_path = magento::fetch_zip_for_title(&client, title, title_dir).await?;
    Ok(plain_from_audio_path(audio_path))
}

async fn fetch_browser(
    store_base_url: &str,
    email: &str,
    password: &str,
    product_id: &str,
    title_dir: &Path,
) -> Result<PlainFetch> {
    let client = MagentoClient::new(store_base_url)?;
    client.login(email, password).await?;
    let audio_path = magento::fetch_browser_audio(&client, product_id, title_dir).await?;
    Ok(plain_from_audio_path(audio_path))
}

async fn fetch_access_app(
    client: &GraphicAudioClient,
    product_id: &str,
    cache_dir: &Path,
    prefer_hi: bool,
) -> Result<PlainFetch> {
    std::fs::create_dir_all(cache_dir)?;
    let title_dir = cache_dir.join(product_id);
    std::fs::create_dir_all(&title_dir)?;

    let links = client.links(product_id).await?;
    let url = links.url_for_quality(prefer_hi).ok_or_else(|| {
        GraphicAudioError::download(format!("no Lo/Hi download URL for product {product_id}"))
    })?;

    let ext = extension_from_url(url);
    let path = title_dir.join(format!("audio{ext}"));
    client.download_to_path(url, &path).await?;

    Ok(PlainFetch {
        parts: vec![PlainAudioPart {
            path,
            title: None,
            duration_ms: None,
        }],
        m4b_path: None,
        cover_path: None,
        chapters: Vec::new(),
    })
}

fn plain_from_audio_path(path: PathBuf) -> PlainFetch {
    let is_m4b = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("m4b"));
    if is_m4b {
        PlainFetch {
            parts: Vec::new(),
            m4b_path: Some(path),
            cover_path: None,
            chapters: Vec::new(),
        }
    } else {
        PlainFetch {
            parts: vec![PlainAudioPart {
                path,
                title: None,
                duration_ms: None,
            }],
            m4b_path: None,
            cover_path: None,
            chapters: Vec::new(),
        }
    }
}

/// Resolve a display title for Magento ZIP matching from Access App products.
pub async fn product_title_for(
    client: &GraphicAudioClient,
    product_id: &str,
) -> Result<Option<String>> {
    let products = client.products().await?;
    Ok(products
        .into_iter()
        .find(|p| p.id == product_id)
        .map(|p: Product| p.display_title()))
}

fn extension_from_url(url: &str) -> &'static str {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    if path.ends_with(".m4b") {
        ".m4b"
    } else if path.ends_with(".m4a") || path.ends_with(".mp4") {
        ".m4a"
    } else if path.ends_with(".zip") {
        ".zip"
    } else {
        ".mp3"
    }
}
