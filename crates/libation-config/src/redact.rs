//! Secret / sensitive-data redaction for log sinks and diagnostics uploads.
//!
//! Redaction applies to **every** sink (stderr, journald, upload payloads). It is
//! not configurable — secrets must never leave the process in cleartext logs.

use std::fmt::{self, Write as _};
use std::sync::OnceLock;

use regex::Regex;
use tracing::field::{Field, Visit};

/// Replacement token used wherever a secret or sensitive value is scrubbed.
pub const REDACTED: &str = "[REDACTED]";

/// Returns true when a tracing field name should never be logged in cleartext.
#[must_use]
pub fn is_sensitive_field(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    // Exact / suffix-style matches common in our crates and HTTP stacks.
    matches!(
        n.as_str(),
        "password"
            | "passwd"
            | "passphrase"
            | "secret"
            | "token"
            | "access_token"
            | "refresh_token"
            | "id_token"
            | "authorization"
            | "cookie"
            | "set_cookie"
            | "api_key"
            | "apikey"
            | "client_secret"
            | "private_key"
            | "aws_secret_access_key"
            | "aws_access_key_id"
            | "secret_access_key"
            | "access_key"
            | "otp"
            | "totp"
            | "mfa_code"
            | "adrm_key"
            | "aax_key"
            | "voucher"
            | "license"
            | "license_response"
            | "widevine_key"
            | "widevine"
            | "cdm"
            | "wvd"
            | "github_token"
            | "gh_token"
    ) || n.contains("password")
        || n.contains("passwd")
        || n.contains("secret")
        || n.ends_with("_token")
        || n.ends_with("_key")
        || n.contains("authorization")
        || n.contains("cookie")
        || n.contains("voucher")
        || n.contains("license")
}

/// Field names scrubbed from **remote** diagnostics uploads (GitHub issues / HTTP)
/// in addition to [`is_sensitive_field`]. Local journal/stderr may still show them.
#[must_use]
pub fn is_upload_identifying_field(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    matches!(
        n.as_str(),
        "title"
            | "subtitle"
            | "author"
            | "authors"
            | "narrator"
            | "narrators"
            | "path"
            | "dest"
            | "password_file"
            | "account"
            | "email"
            | "username"
            | "user"
    ) || n.contains("password_file")
        || n.ends_with("_path")
}

/// Scrub known secret patterns from an arbitrary string (messages, Debug output, URLs).
#[must_use]
pub fn redact_str(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut out = input.to_string();
    for re in secret_patterns() {
        out = re.replace_all(&out, REDACTED).into_owned();
    }
    out
}

/// Redact a field value: sensitive names are fully replaced; others are pattern-scrubbed.
#[must_use]
pub fn redact_field_value(name: &str, value: &str) -> String {
    if is_sensitive_field(name) {
        REDACTED.to_string()
    } else {
        redact_str(value)
    }
}

/// Extra sanitization for payloads that leave the machine (GitHub issues / HTTP).
#[must_use]
pub fn sanitize_for_remote_upload(name: &str, value: &str) -> String {
    if is_sensitive_field(name) || is_upload_identifying_field(name) {
        return REDACTED.to_string();
    }
    let mut out = redact_str(value);
    out = redact_home_paths(&out);
    out = redact_auth_paths(&out);
    out
}

fn redact_home_paths(input: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)(/home/|/Users/|\\Users\\)([^/\\]+)").expect("home path regex")
    });
    re.replace_all(input, |caps: &regex::Captures| {
        format!("{}{}", &caps[1], REDACTED)
    })
    .into_owned()
}

fn redact_auth_paths(input: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)(Accounts[/\\])([^/\\]+)(\.auth|\.wvd)").expect("auth path regex")
    });
    re.replace_all(input, |caps: &regex::Captures| {
        format!("{}{}{}", &caps[1], REDACTED, &caps[3])
    })
    .into_owned()
}

fn secret_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        const SOURCES: &[&str] = &[
            // Audible / Amazon token shapes seen in `.auth` / API fixtures.
            r"(?i)\bAtna\|[A-Za-z0-9._\-+/=]+",
            r"(?i)\bAtnr\|[A-Za-z0-9._\-+/=]+",
            r"(?i)\bBearer\s+[A-Za-z0-9._\-+/=]+",
            // AWS access key id + common secret-key length blobs next to aws_secret.
            r"\bAKIA[0-9A-Z]{16}\b",
            r"(?i)(aws_secret_access_key\s*[=:]\s*)\S+",
            // GitHub tokens (classic + fine-grained prefixes).
            r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b",
            r"\bgithub_pat_[A-Za-z0-9_]{20,}\b",
            // Long URL query secrets.
            r"(?i)([?&](?:password|passwd|token|access_token|refresh_token|api_key|key|secret)=)[^&\s]+",
            // PEM blocks / Widevine-ish base64 blobs labeled as keys.
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            r"(?i)(otp|totp|mfa)([=: ])\S+",
        ];
        SOURCES
            .iter()
            .map(|s| Regex::new(s).expect("static redaction regex"))
            .collect()
    })
}

/// Collects event fields into a map while applying redaction.
#[derive(Default)]
pub struct RedactingVisitor {
    pub message: Option<String>,
    pub fields: Vec<(String, String)>,
}

impl Visit for RedactingVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_display(field, &value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_display(field, &value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_display(field, &value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_display(field, &value);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        let redacted = redact_field_value(name, value);
        if name == "message" {
            self.message = Some(redacted);
        } else {
            self.fields.push((name.to_string(), redacted));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let mut raw = String::new();
        let _ = write!(raw, "{value:?}");
        let name = field.name();
        let redacted = redact_field_value(name, &raw);
        if name == "message" {
            self.message = Some(redacted);
        } else {
            self.fields.push((name.to_string(), redacted));
        }
    }
}

impl RedactingVisitor {
    fn record_display(&mut self, field: &Field, value: &dyn fmt::Display) {
        let raw = value.to_string();
        let name = field.name();
        let redacted = redact_field_value(name, &raw);
        if name == "message" {
            self.message = Some(redacted);
        } else {
            self.fields.push((name.to_string(), redacted));
        }
    }
}

/// [`std::io::Write`] wrapper that runs [`redact_str`] on every chunk before forwarding.
///
/// Used so the stderr `fmt` subscriber cannot emit cleartext secrets even when a
/// call site formats a token into the message body.
pub struct RedactingWriter<W> {
    inner: W,
}

impl<W> RedactingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: std::io::Write> std::io::Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Preserve reported write length as the caller's byte count so partial
        // UTF-8 across chunk boundaries still advances the fmt writer correctly.
        let len = buf.len();
        match std::str::from_utf8(buf) {
            Ok(s) => {
                let redacted = redact_str(s);
                self.inner.write_all(redacted.as_bytes())?;
            }
            Err(_) => {
                // Non-UTF8 chunks are rare for tracing fmt; drop rather than risk secrets.
                self.inner.write_all(REDACTED.as_bytes())?;
            }
        }
        Ok(len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_field_names() {
        assert!(is_sensitive_field("password"));
        assert!(is_sensitive_field("refresh_token"));
        assert!(is_sensitive_field("AWS_SECRET_ACCESS_KEY"));
        assert!(is_sensitive_field("client_secret"));
        assert!(!is_sensitive_field("asin"));
        assert!(!is_sensitive_field("title"));
    }

    #[test]
    fn redacts_audible_tokens_in_message() {
        let s = "login failed token=Atna|abcDEF123 refresh=Atnr|xyz";
        let out = redact_str(s);
        assert!(!out.contains("Atna|abcDEF123"));
        assert!(!out.contains("Atnr|xyz"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn redacts_bearer_and_aws() {
        let s = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.xx AWS=AKIAIOSFODNN7EXAMPLE";
        let out = redact_str(s);
        assert!(!out.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn sensitive_field_fully_redacted() {
        assert_eq!(
            redact_field_value("refresh_token", "Atnr|not-a-real-token"),
            REDACTED
        );
        assert_eq!(
            redact_field_value("asin", "B00TEST123"),
            "B00TEST123".to_string()
        );
    }

    #[test]
    fn writer_scrubs_secrets() {
        let mut buf = Vec::new();
        {
            let mut w = RedactingWriter::new(&mut buf);
            use std::io::Write;
            write!(w, "got Bearer super-secret-token-value ok").unwrap();
        }
        let s = String::from_utf8(buf).unwrap();
        assert!(!s.contains("super-secret-token-value"));
        assert!(s.contains(REDACTED));
    }

    #[test]
    fn sanitize_strips_titles_and_home_paths() {
        assert_eq!(sanitize_for_remote_upload("title", "My Book"), REDACTED);
        let path = sanitize_for_remote_upload("message", "/home/alice/Accounts/bob.auth");
        assert!(!path.contains("alice"));
        assert!(path.contains(REDACTED));
    }
}
