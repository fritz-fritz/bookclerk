//! Auth-file encryption passphrase resolution for headless deployments.
//!
//! Audible OAuth material is stored as audible-rs envelopes under `Accounts/`,
//! always encrypted (Argon2id + XChaCha20-Poly1305). Passphrase resolution:
//!
//! 1. `LIBATION_AUTH_PASSWORD` (env)
//! 2. `LIBATION_AUTH_PASSWORD_FILE` (env path — Docker/systemd secret)
//! 3. Optional config `[auth].password_file`
//! 4. Managed default: `{files_dir}/Accounts/.encryption_key`
//!    (created with a strong CSPRNG secret on first use)
//!
//! OS keychains are a poor fit for non-interactive VPS/Docker.

use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};

use crate::error::{AudibleError, Result};
use crate::paths::{accounts_dir, ensure_accounts_dir};

/// Environment variable holding the auth-file passphrase directly.
pub const AUTH_PASSWORD_ENV: &str = "LIBATION_AUTH_PASSWORD";

/// Environment variable pointing at a file that contains the passphrase.
pub const AUTH_PASSWORD_FILE_ENV: &str = "LIBATION_AUTH_PASSWORD_FILE";

/// Filename for the auto-generated shared passphrase under `Accounts/`.
pub const MANAGED_ENCRYPTION_KEY_NAME: &str = ".encryption_key";

/// Path to the managed default passphrase file.
#[must_use]
pub fn managed_encryption_key_path(files_dir: &Path) -> PathBuf {
    accounts_dir(files_dir).join(MANAGED_ENCRYPTION_KEY_NAME)
}

/// Resolve the auth-file encryption passphrase (always returns a secret).
///
/// When no explicit passphrase is configured, ensures
/// `Accounts/.encryption_key` exists (generating a 256-bit random secret).
pub fn resolve_auth_password(
    files_dir: &Path,
    configured_password_file: Option<&Path>,
) -> Result<SecretString> {
    if let Ok(value) = std::env::var(AUTH_PASSWORD_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(SecretString::from(trimmed.to_string()));
        }
    }

    if let Ok(path) = std::env::var(AUTH_PASSWORD_FILE_ENV) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return read_password_file(Path::new(trimmed));
        }
    }

    if let Some(path) = configured_password_file {
        return read_password_file(path);
    }

    ensure_managed_encryption_key(files_dir)
}

/// Ensure `Accounts/.encryption_key` exists; generate one if missing.
pub fn ensure_managed_encryption_key(files_dir: &Path) -> Result<SecretString> {
    ensure_accounts_dir(files_dir).map_err(|err| {
        AudibleError::Auth(format!("failed to create Accounts directory: {err}"))
    })?;
    let path = managed_encryption_key_path(files_dir);
    if path.is_file() {
        return read_password_file(&path);
    }

    let secret = generate_strong_password()?;
    write_password_file(&path, secret.expose_secret())?;
    tracing::info!(
        path = %path.display(),
        "generated Accounts/.encryption_key for auth-file encryption \
         (override with LIBATION_AUTH_PASSWORD or a password file if desired)"
    );
    Ok(secret)
}

fn generate_strong_password() -> Result<SecretString> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|err| {
        AudibleError::Auth(format!("failed to generate auth encryption key: {err}"))
    })?;
    Ok(SecretString::from(encode_hex(&bytes)))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
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

fn write_password_file(path: &Path, secret: &str) -> Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).map_err(|err| {
        AudibleError::Auth(format!(
            "failed to create auth encryption key {}: {err}",
            path.display()
        ))
    })?;
    file.write_all(secret.as_bytes()).map_err(|err| {
        AudibleError::Auth(format!(
            "failed to write auth encryption key {}: {err}",
            path.display()
        ))
    })?;
    file.write_all(b"\n").ok();
    harden_secret_path(path)?;
    Ok(())
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(AUTH_PASSWORD_ENV, "unit-test-secret");
        std::env::remove_var(AUTH_PASSWORD_FILE_ENV);
        let dir = tempfile::tempdir().unwrap();
        let got = resolve_auth_password(dir.path(), None).unwrap();
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
        let got = resolve_auth_password(dir.path(), Some(&path)).unwrap();
        assert_eq!(got.expose_secret(), "file-secret");
    }

    #[test]
    fn generates_managed_key_once() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(AUTH_PASSWORD_ENV);
        std::env::remove_var(AUTH_PASSWORD_FILE_ENV);
        let dir = tempfile::tempdir().unwrap();
        let first = resolve_auth_password(dir.path(), None).unwrap();
        let second = resolve_auth_password(dir.path(), None).unwrap();
        assert_eq!(first.expose_secret(), second.expose_secret());
        assert_eq!(first.expose_secret().len(), 64);
        assert!(managed_encryption_key_path(dir.path()).is_file());
    }
}
