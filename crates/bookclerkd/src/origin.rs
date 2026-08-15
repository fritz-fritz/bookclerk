//! Public origin used for OIDC issuer, SSO callbacks, and invite URLs.
//!
//! Prefers `integrations.public_origin`, then the request `Origin` / `Host`,
//! then `http://localhost:{listen_port}`. Loopback IPs are rewritten to
//! `localhost` so tray/`cargo dev` URLs match WebAuthn RP IDs.

use axum::http::{header, HeaderMap};
use bookclerk_config::{Config, ListenAddrs};
use url::Url;

/// Trimmed configured origin with no trailing slash, or `None` when unset.
#[must_use]
pub fn configured_public_origin(public_origin: Option<&str>) -> Option<String> {
    public_origin
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/').to_string())
}

/// Rewrite loopback IPs (`127.0.0.1`, `::1`) to `localhost`, keeping scheme/port.
#[must_use]
pub fn rewrite_loopback_host(origin: &str) -> String {
    let trimmed = origin.trim().trim_end_matches('/');
    let Ok(url) = Url::parse(trimmed) else {
        return trimmed.to_string();
    };
    let loopback = match url.host() {
        Some(url::Host::Ipv4(addr)) if addr.is_loopback() => true,
        Some(url::Host::Ipv6(addr)) if addr.is_loopback() => true,
        _ => false,
    };
    if !loopback {
        return trimmed.to_string();
    }
    match url.port() {
        Some(port) => format!("{}://localhost:{port}", url.scheme()),
        None => format!("{}://localhost", url.scheme()),
    }
}

/// Request `Origin` when it is a usable absolute origin (not `null`).
#[must_use]
pub fn request_origin(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::ORIGIN)?.to_str().ok()?.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("null") {
        return None;
    }
    Some(raw.trim_end_matches('/').to_string())
}

/// Origin inferred from `Host` (and optional `X-Forwarded-Proto`).
#[must_use]
pub fn origin_from_host(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(header::HOST)?.to_str().ok()?.trim();
    if host.is_empty() {
        return None;
    }
    let forwarded = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let scheme = if let Some(proto) = forwarded {
        proto
    } else if host.contains("localhost") || host.starts_with("127.") || host.starts_with('[') {
        "http"
    } else {
        "https"
    };
    Some(format!("{scheme}://{host}"))
}

/// Loopback issuer when nothing is configured or detected.
#[must_use]
pub fn loopback_fallback(listen: &ListenAddrs) -> String {
    format!("http://localhost:{}", listen.ui_port())
}

/// Origin detected from this request, rewritten to `localhost` on loopback IPs.
#[must_use]
pub fn detected_origin(headers: &HeaderMap, listen: &ListenAddrs) -> String {
    let raw = request_origin(headers)
        .or_else(|| origin_from_host(headers))
        .unwrap_or_else(|| loopback_fallback(listen));
    rewrite_loopback_host(&raw)
}

/// Effective public origin: pinned config, else this request, else localhost.
#[must_use]
pub fn effective_public_origin(
    public_origin: Option<&str>,
    headers: Option<&HeaderMap>,
    listen: &ListenAddrs,
) -> String {
    if let Some(configured) = configured_public_origin(public_origin) {
        return rewrite_loopback_host(&configured);
    }
    if let Some(headers) = headers {
        return detected_origin(headers, listen);
    }
    loopback_fallback(listen)
}

/// Effective origin from a live [`Config`] plus optional request headers.
#[must_use]
pub fn effective_origin_from_config(cfg: &Config, headers: Option<&HeaderMap>) -> String {
    effective_public_origin(
        cfg.integrations.public_origin.as_deref(),
        headers,
        &cfg.daemon.listen,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn rewrites_loopback_ip_to_localhost() {
        assert_eq!(
            rewrite_loopback_host("http://127.0.0.1:8787"),
            "http://localhost:8787"
        );
        assert_eq!(
            rewrite_loopback_host("http://[::1]:8787"),
            "http://localhost:8787"
        );
        assert_eq!(
            rewrite_loopback_host("https://bookclerk.example.com"),
            "https://bookclerk.example.com"
        );
    }

    #[test]
    fn pinned_origin_wins_over_request() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://localhost:5173"),
        );
        let listen = ListenAddrs::parse_list("127.0.0.1:8787").unwrap();
        assert_eq!(
            effective_public_origin(
                Some("https://bookclerk.example.com"),
                Some(&headers),
                &listen
            ),
            "https://bookclerk.example.com"
        );
    }

    #[test]
    fn unset_origin_uses_request_then_localhost() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:8787"),
        );
        let listen = ListenAddrs::parse_list("127.0.0.1:8787").unwrap();
        assert_eq!(
            effective_public_origin(None, Some(&headers), &listen),
            "http://localhost:8787"
        );
        assert_eq!(
            effective_public_origin(None, None, &listen),
            "http://localhost:8787"
        );
    }
}
