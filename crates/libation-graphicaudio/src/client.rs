//! HTTP client for the GraphicAudio Android Retrofit API.

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::{GraphicAudioError, Result};

/// Default API origin (no trailing slash after `/access`).
pub const DEFAULT_BASE_URL: &str = "https://www.graphicaudio.net/access";

/// Device activation / password login.
pub const LOGIN_PATH: &str = "/activation/login";

/// Library + sample products listing.
pub const PRODUCTS_PATH: &str = "/api/products";

/// Download URL lookup (`?product=`).
pub const LINKS_PATH: &str = "/api/links";

const USER_AGENT_VALUE: &str = "okhttp/4.12.0 GraphicAudio/Libation";

/// Authenticated GraphicAudio HTTP helper.
#[derive(Debug, Clone)]
pub struct GraphicAudioClient {
    http: Client,
    base_url: String,
    token: Option<String>,
}

impl Default for GraphicAudioClient {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL)
    }
}

impl GraphicAudioClient {
    /// Build a client pointed at `base_url` (trailing slash stripped).
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            http: Client::new(),
            base_url,
            token: None,
        }
    }

    /// Override the HTTP client (tests / custom timeouts).
    #[must_use]
    pub fn with_http(mut self, http: Client) -> Self {
        self.http = http;
        self
    }

    /// Attach an activation token for subsequent requests.
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Current token, if any.
    #[must_use]
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Base URL in use.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn headers(&self, with_auth: bool) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("application/json"),
        );
        if with_auth {
            let token = self
                .token
                .as_deref()
                .ok_or_else(|| GraphicAudioError::auth("not logged in"))?;
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(token)
                    .map_err(|e| GraphicAudioError::auth(format!("invalid token header: {e}")))?,
            );
        }
        Ok(headers)
    }

    /// `POST activation/login` — returns the opaque token string.
    pub async fn login(
        &mut self,
        username: &str,
        password: &str,
        client_id: &str,
    ) -> Result<String> {
        let url = format!("{}{LOGIN_PATH}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .headers(self.headers(false)?)
            .form(&[
                ("username", username),
                ("password", password),
                ("client_id", client_id),
            ])
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            let msg = serde_json::from_str::<LoginErrorBody>(&body)
                .ok()
                .and_then(|e| e.message.or(e.title))
                .unwrap_or(body);
            return Err(GraphicAudioError::auth(format!(
                "login failed ({status}): {msg}"
            )));
        }
        let parsed: LoginOk = serde_json::from_str(&body)?;
        let token = parsed
            .token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| GraphicAudioError::auth("login response missing token"))?;
        self.token = Some(token.clone());
        Ok(token)
    }

    /// `GET api/products` — owned titles plus promotional samples.
    pub async fn products(&self) -> Result<Vec<Product>> {
        let url = format!("{}{PRODUCTS_PATH}", self.base_url);
        let resp = self
            .http
            .get(&url)
            .headers(self.headers(true)?)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(GraphicAudioError::api(format!(
                "products failed ({status}): {body}"
            )));
        }
        Ok(serde_json::from_str(&body)?)
    }

    /// `GET api/links?product=` — Lo/Hi plain media URLs.
    pub async fn links(&self, product_id: &str) -> Result<DownloadLinks> {
        let url = format!("{}{LINKS_PATH}", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("product", product_id)])
            .headers(self.headers(true)?)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(GraphicAudioError::api(format!(
                "links failed ({status}): {body}"
            )));
        }
        Ok(serde_json::from_str(&body)?)
    }

    /// Download bytes from an absolute media URL (no Authorization).
    pub async fn download_bytes(&self, url: &str) -> Result<bytes::Bytes> {
        let resp = self
            .http
            .get(url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(GraphicAudioError::download(format!(
                "download failed ({status}) for {url}"
            )));
        }
        Ok(resp.bytes().await?)
    }
}

#[derive(Debug, Deserialize)]
struct LoginOk {
    #[serde(alias = "Token")]
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginErrorBody {
    #[serde(alias = "Message")]
    message: Option<String>,
    #[serde(alias = "Title")]
    title: Option<String>,
}

/// One library / sample product from `api/products`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Type", default)]
    pub product_type: Option<String>,
    #[serde(rename = "ProductName", default)]
    pub product_name: Option<String>,
    #[serde(rename = "Title", default)]
    pub title: Option<String>,
    #[serde(rename = "Author", default)]
    pub author: Option<String>,
    #[serde(rename = "Series", default)]
    pub series: Option<String>,
    #[serde(rename = "Episode", default)]
    pub episode: Option<String>,
    #[serde(rename = "Genre", default)]
    pub genre: Option<String>,
    #[serde(rename = "Image", default)]
    pub image: Option<String>,
    #[serde(rename = "Thumbnail", default)]
    pub thumbnail: Option<String>,
    #[serde(rename = "Running Time", default)]
    pub running_time: Option<String>,
    #[serde(rename = "Purchased Date", default)]
    pub purchased_date: Option<String>,
    #[serde(rename = "Release Date", default)]
    pub release_date: Option<String>,
}

impl Product {
    /// Whether this entry is a promotional sample (not owned).
    #[must_use]
    pub fn is_sample(&self) -> bool {
        self.product_type
            .as_deref()
            .map(|t| t.eq_ignore_ascii_case("sample"))
            .unwrap_or(false)
    }

    /// Display title preferring `ProductName`.
    #[must_use]
    pub fn display_title(&self) -> String {
        self.product_name
            .as_deref()
            .or(self.title.as_deref())
            .unwrap_or(self.id.as_str())
            .to_string()
    }
}

/// Lo/Hi download URLs from `api/links`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadLinks {
    #[serde(rename = "Lo", default)]
    pub lo: Option<String>,
    #[serde(rename = "Hi", default)]
    pub hi: Option<String>,
}

impl DownloadLinks {
    /// Prefer high-quality URL, else low.
    #[must_use]
    pub fn preferred_url(&self) -> Option<&str> {
        self.hi
            .as_deref()
            .filter(|u| !u.is_empty())
            .or_else(|| self.lo.as_deref().filter(|u| !u.is_empty()))
    }
}
