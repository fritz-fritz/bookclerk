//! Host-side guard: keep secrets out of unprotected (plaintext) DB columns.
//!
//! Credentials belong in `encrypted_secrets` (`sealed-v1`). Freeform fields such
//! as `books.title`, `books.error_message`, and listening-progress metadata must
//! never store registered secrets or well-known secret shapes (Audible tokens,
//! AWS keys, private keys, …).
//!
//! Policy:
//! 1. Scrub with [`bookclerk_config::redact_str`] (exact registered values + patterns).
//! 2. Fail closed if a registered secret is still present after scrubbing.
//! 3. Return the scrubbed text (callers persist that, never the original).

use bookclerk_config::{contains_registered_secret, redact_str, REDACTED};

use crate::error::{LibraryError, Result};

/// True when `text` contains a registered secret or a known secret shape.
#[must_use]
pub fn looks_like_secret_leak(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    contains_registered_secret(text) || redact_str(text) != text
}

/// Scrub `value` for persistence in an unprotected DB column.
///
/// Returns the scrubbed string. Errors if a registered secret would still be
/// stored (fail closed — same posture as diagnostics upload abort).
pub fn guard_unprotected_text(field: &str, value: &str) -> Result<String> {
    if value.is_empty() {
        return Ok(String::new());
    }
    let had_leak = looks_like_secret_leak(value);
    let scrubbed = redact_str(value);
    if contains_registered_secret(&scrubbed) {
        return Err(LibraryError::SecretLeak(format!(
            "refusing to persist registered secret in unprotected field `{field}`"
        )));
    }
    if had_leak {
        tracing::warn!(
            field,
            redacted = REDACTED,
            "scrubbed secret material from unprotected database field"
        );
    }
    Ok(scrubbed)
}

/// Scrub an optional unprotected field; `None` stays `None`.
pub fn guard_unprotected_optional(field: &str, value: Option<&str>) -> Result<Option<String>> {
    match value {
        None => Ok(None),
        Some(v) => Ok(Some(guard_unprotected_text(field, v)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_aws_access_key_pattern() {
        let out =
            guard_unprotected_text("error_message", "failed with AKIAIOSFODNN7EXAMPLE").unwrap();
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(out.contains(REDACTED));
        assert!(looks_like_secret_leak("failed with AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn scrubs_audible_refresh_token_shape() {
        let out = guard_unprotected_text(
            "error_message",
            "auth failed Atnr|AbCdEf1234567890._-+/=rest",
        )
        .unwrap();
        assert!(!out.contains("Atnr|"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn scrubs_bearer_token() {
        let out = guard_unprotected_text(
            "error_message",
            "upstream said Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload",
        )
        .unwrap();
        assert!(!out.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn plain_title_unchanged() {
        let title = "The Name of the Wind";
        assert_eq!(guard_unprotected_text("title", title).unwrap(), title);
        assert!(!looks_like_secret_leak(title));
    }
}
