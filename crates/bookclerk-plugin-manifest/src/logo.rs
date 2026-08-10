//! Plugin logo validation: remote http(s) URL or relative image path.

use crate::error::{Error, Result};

/// Allowed image extensions for embedded logos (lowercase, with dot).
pub const LOGO_EXTENSIONS: &[&str] = &[".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".ico"];

/// Maximum size when the host serves an embedded logo.
pub const MAX_EMBEDDED_LOGO_BYTES: u64 = 512 * 1024;

/// Classified `plugin.toml` `logo` value after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogoKind {
    /// Browser-loadable `http://` or `https://` URL.
    RemoteUrl(String),
    /// Path relative to the plugin install root (host serves via API).
    EmbeddedPath(String),
}

impl LogoKind {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::RemoteUrl(s) | Self::EmbeddedPath(s) => s.as_str(),
        }
    }

    #[must_use]
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::RemoteUrl(_))
    }

    #[must_use]
    pub fn is_embedded(&self) -> bool {
        matches!(self, Self::EmbeddedPath(_))
    }
}

/// Validate an optional logo string from `plugin.toml`.
pub fn validate_logo(raw: &str) -> Result<LogoKind> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::message(
            "plugin.toml: `logo` must not be empty (omit the key instead)",
        ));
    }
    if trimmed.contains('\0') {
        return Err(Error::message("plugin.toml: `logo` must not contain NUL"));
    }
    if looks_like_url(trimmed) {
        return validate_remote_url(trimmed);
    }
    validate_embedded_path(trimmed)
}

fn looks_like_url(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.contains("://") || lower.starts_with("javascript:") || lower.starts_with("data:")
}

fn validate_remote_url(s: &str) -> Result<LogoKind> {
    let lower = s.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(Error::message(
            "plugin.toml: `logo` URL must use http:// or https:// (no javascript:/data:/file:)",
        ));
    }
    let after_scheme = if let Some(rest) = s.strip_prefix("https://") {
        rest
    } else if let Some(rest) = s.strip_prefix("http://") {
        rest
    } else if let Some(rest) = s.strip_prefix("HTTPS://") {
        rest
    } else if let Some(rest) = s.strip_prefix("HTTP://") {
        rest
    } else {
        return Err(Error::message(
            "plugin.toml: `logo` URL must use http:// or https://",
        ));
    };
    if after_scheme.is_empty() {
        return Err(Error::message("plugin.toml: `logo` URL is missing a host"));
    }
    // Reject userinfo (user:pass@host) — common injection / phishing vector.
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err(Error::message(
            "plugin.toml: `logo` URL must not include userinfo (user:pass@host)",
        ));
    }
    let host = authority
        .split('%')
        .next()
        .unwrap_or(authority)
        .split(':')
        .next()
        .unwrap_or("")
        .trim();
    if host.is_empty() || host == "." || host == ".." {
        return Err(Error::message("plugin.toml: `logo` URL is missing a host"));
    }
    Ok(LogoKind::RemoteUrl(s.to_string()))
}

fn validate_embedded_path(s: &str) -> Result<LogoKind> {
    let path = s.replace('\\', "/");
    if path.starts_with('/') || path.starts_with('~') {
        return Err(Error::message(
            "plugin.toml: embedded `logo` must be a relative path under the plugin root",
        ));
    }
    // Windows drive / UNC
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return Err(Error::message(
            "plugin.toml: embedded `logo` must be a relative path (no drive letter)",
        ));
    }
    if path.starts_with("//") {
        return Err(Error::message(
            "plugin.toml: embedded `logo` must be a relative path (no UNC)",
        ));
    }
    let mut segments = Vec::new();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return Err(Error::message(
                "plugin.toml: embedded `logo` must not contain `..` segments",
            ));
        }
        segments.push(seg);
    }
    if segments.is_empty() {
        return Err(Error::message(
            "plugin.toml: embedded `logo` path is empty after normalization",
        ));
    }
    let normalized = segments.join("/");
    let lower = normalized.to_ascii_lowercase();
    if !LOGO_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return Err(Error::message(format!(
            "plugin.toml: embedded `logo` must end with one of {}",
            LOGO_EXTENSIONS.join(", ")
        )));
    }
    Ok(LogoKind::EmbeddedPath(normalized))
}

/// MIME type for an embedded logo path (by extension).
#[must_use]
pub fn logo_content_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".ico") {
        "image/x-icon"
    } else {
        "application/octet-stream"
    }
}

/// Settings / UI href for an embedded logo (`/api/plugins/{kind}/{id}/logo`).
#[must_use]
pub fn embedded_logo_api_path(kind: &str, id: &str) -> String {
    format!("/api/plugins/{kind}/{id}/logo")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_favicon() {
        let k =
            validate_logo("https://www.google.com/s2/favicons?domain=audible.com&sz=128").unwrap();
        assert!(k.is_remote());
    }

    #[test]
    fn rejects_javascript() {
        let err = validate_logo("javascript:alert(1)").unwrap_err();
        assert!(err.to_string().contains("http"), "{err}");
    }

    #[test]
    fn rejects_data_url() {
        let err = validate_logo("data:image/png;base64,aaaa").unwrap_err();
        assert!(err.to_string().contains("http"), "{err}");
    }

    #[test]
    fn rejects_userinfo() {
        let err = validate_logo("https://evil:pw@example.com/x.png").unwrap_err();
        assert!(err.to_string().contains("userinfo"), "{err}");
    }

    #[test]
    fn accepts_relative_png() {
        let k = validate_logo("assets/logo.png").unwrap();
        assert_eq!(k, LogoKind::EmbeddedPath("assets/logo.png".into()));
    }

    #[test]
    fn rejects_parent_segments() {
        let err = validate_logo("../etc/passwd.png").unwrap_err();
        assert!(err.to_string().contains(".."), "{err}");
    }

    #[test]
    fn rejects_absolute_path() {
        assert!(validate_logo("/etc/logo.png").is_err());
    }
}
