//! Auth-file encryption passphrase resolution for headless deployments.
//!
//! Audible OAuth material is stored as audible-rs envelopes under `Accounts/`.
//! Prefer encrypting those envelopes with a shared passphrase from:
//!
//! 1. `LIBATION_AUTH_PASSWORD` (env)
//! 2. `LIBATION_AUTH_PASSWORD_FILE` (env path to a secret file — Docker/systemd)
//! 3. Optional config `[auth].password_file`
//!
//! OS keychains are a poor fit for non-interactive VPS/Docker; hashing alone
//! cannot recover tokens for refresh. audible-rs already uses Argon2id +
//! XChaCha20-Poly1305 for the envelope.

use std::path::Path;

use secrecy::SecretString;

use crate::error::{AudibleError, Result};

/// Environment variable holding the auth-file passphrase directly.
pub const AUTH_PASSWORD_ENV: &str = "LIBATION_AUTH_PASSWORD";

/// Environment variable pointing at a file that contains the passphrase.
pub const AUTH_PASSWORD_FILE_ENV: &str = "LIBATION_AUTH_PASSWORD_FILE";

/// Resolve the auth-file encryption passphrase.
///
/// `configured_password_file` comes from `[auth].password_file` when available.
pub fn resolve_auth_password(
    configured_password_file: Option<&Path>,
) -> Result<Option<SecretString>> {
    if let Ok(value) = std::env::var(AUTH_PASSWORD_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(Some(SecretString::from(trimmed.to_string())));
        }
    }

    if let Ok(path) = std::env::var(AUTH_PASSWORD_FILE_ENV) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(Some(read_password_file(Path::new(trimmed))?));
        }
    }

    if let Some(path) = configured_password_file {
        return Ok(Some(read_password_file(path)?));
    }

    Ok(None)
}

/// Require a passphrase for writing encrypted auth files.
pub fn require_auth_password(configured_password_file: Option<&Path>) -> Result<SecretString> {
    resolve_auth_password(configured_password_file)?.ok_or_else(|| {
        AudibleError::Auth(format!(
            "auth-file encryption requires a passphrase — set {AUTH_PASSWORD_ENV}, \
             {AUTH_PASSWORD_FILE_ENV}, or [auth].password_file (or set \
             auth.allow_plaintext = true to store unprotected tokens)"
        ))
    })
}

fn read_password_file(path: &Path) -> Result<SecretString> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        AudibleError::Auth(format!(
            "failed to read auth password file {}: {err}",
            path.display()
        ))
    })?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AudibleError::Auth(format!(
            "auth password file {} is empty",
            path.display()
        )));
    }
    Ok(SecretString::from(trimmed.to_string()))
}

/// Best-effort restrictive permissions for a secret-bearing path (Unix).
pub fn harden_secret_path(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.is_dir() {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        } else if path.is_file() {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(AUTH_PASSWORD_ENV, "unit-test-secret");
        std::env::remove_var(AUTH_PASSWORD_FILE_ENV);
        let got = resolve_auth_password(None).unwrap().unwrap();
        assert_eq!(got.expose_secret(), "unit-test-secret");
        std::env::remove_var(AUTH_PASSWORD_ENV);
    }

    #[test]
    fn resolve_from_password_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(AUTH_PASSWORD_ENV);
        std::env::remove_var(AUTH_PASSWORD_FILE_ENV);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pw");
        std::fs::write(&path, "file-secret\n").unwrap();
        let got = resolve_auth_password(Some(&path)).unwrap().unwrap();
        assert_eq!(got.expose_secret(), "file-secret");
    }
}
