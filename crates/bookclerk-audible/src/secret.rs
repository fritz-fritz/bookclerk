//! Auth encryption passphrase resolution for headless deployments.
//!
//! Audible OAuth material (and other source / destination secrets) live in the
//! `encrypted_secrets` DB table. Encrypt those rows with a passphrase from
//! `BOOKCLERK_AUTH_PASSWORD` (env-only bootstrap — never stored in the DB).
//!
//! With no passphrase configured, callers may opt into unencrypted DB secrets
//! via `auth.allow_plaintext` / [`configure_auth_secrets`].

use std::sync::{Mutex, OnceLock};

use secrecy::SecretString;

use crate::error::{AudibleError, Result};

/// Environment variable holding the auth encryption passphrase.
pub const AUTH_PASSWORD_ENV: &str = "BOOKCLERK_AUTH_PASSWORD";

#[derive(Debug, Clone, Default)]
struct AuthSecretDefaults {
    allow_plaintext: bool,
}

fn auth_defaults() -> &'static Mutex<AuthSecretDefaults> {
    static CELL: OnceLock<Mutex<AuthSecretDefaults>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(AuthSecretDefaults::default()))
}

/// Apply `[auth]` settings from the loaded config so scan/acquire/migrate pick
/// up `allow_plaintext` without threading it through every API.
pub fn configure_auth_secrets(allow_plaintext: bool) {
    if let Ok(mut guard) = auth_defaults().lock() {
        guard.allow_plaintext = allow_plaintext;
    }
}

/// Whether plaintext secrets are allowed when no passphrase is configured.
#[must_use]
pub fn default_allow_plaintext() -> bool {
    auth_defaults()
        .lock()
        .map(|g| g.allow_plaintext)
        .unwrap_or(false)
}

/// Resolve the auth encryption passphrase from `BOOKCLERK_AUTH_PASSWORD`, if set.
pub fn resolve_auth_password() -> Result<Option<SecretString>> {
    if let Ok(value) = std::env::var(AUTH_PASSWORD_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            bookclerk_config::register_secret(trimmed);
            return Ok(Some(SecretString::from(trimmed.to_string())));
        }
    }
    Ok(None)
}

/// Require a passphrase for writing encrypted secrets.
pub fn require_auth_password() -> Result<SecretString> {
    resolve_auth_password()?.ok_or_else(|| {
        AudibleError::Auth(format!(
            "secret encryption requires a passphrase — set {AUTH_PASSWORD_ENV}, \
             or set auth.allow_plaintext = true to store unprotected credentials"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_defaults() {
        configure_auth_secrets(false);
    }

    #[test]
    fn resolve_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_defaults();
        std::env::set_var(AUTH_PASSWORD_ENV, "unit-test-secret");
        let got = resolve_auth_password().unwrap().unwrap();
        assert_eq!(got.expose_secret(), "unit-test-secret");
        std::env::remove_var(AUTH_PASSWORD_ENV);
    }

    #[test]
    fn none_when_unconfigured() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_defaults();
        std::env::remove_var(AUTH_PASSWORD_ENV);
        assert!(resolve_auth_password().unwrap().is_none());
    }
}
