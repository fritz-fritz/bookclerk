//! Data-encryption key (DEK) for `encrypted_secrets`.
//!
//! # Design
//!
//! Secrets in the DB are sealed with a **random 32-byte DEK** + per-row
//! XChaCha20-Poly1305 nonce (format `sealed-v1`). The DEK is **not** derived
//! per secret (no Argon2 on every load/save).
//!
//! ## Bootstrap (outside the DB)
//!
//! | Source | Behavior |
//! | --- | --- |
//! | `BOOKCLERK_AUTH_PASSWORD` | Wraps/unwraps the DEK in `{files_dir}/master.key` |
//! | Auto-mint | If neither env nor file exists, generates a DEK and writes `master.key` (mode `0600`) |
//!
//! The operator API token is **independent** and short-lived for HTTP auth only.
//! Rotating it does **not** re-encrypt library secrets.
//!
//! Prefer a single opaque ciphertext blob per secret (not per-field columns):
//! Audible envelopes are already opaque, updates stay atomic, and one unseal
//! recovers the whole credential.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use argon2::{Algorithm, Argon2, Params as ArgonParams, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{LibraryError, Result};
use crate::secrets::{CIPHER_ALGORITHM, KDF_ALGORITHM, KDF_M_COST, KDF_P_COST, KDF_T_COST};

/// Env var that wraps the DEK in `master.key` (optional).
pub const AUTH_PASSWORD_ENV: &str = "BOOKCLERK_AUTH_PASSWORD";

/// Bootstrap key file under `BOOKCLERK_FILES_DIR` (auto-minted when missing).
pub const MASTER_KEY_FILE_NAME: &str = "master.key";

const MAGIC_RAW: &[u8; 4] = b"BCK1"; // raw DEK (no password wrap)
const MAGIC_WRAPPED: &[u8; 4] = b"BCK2"; // Argon2id + XChaCha wrap
const DEK_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;

/// Process-wide cached DEK (filled by [`configure_master_key`] / [`resolve_master_key`]).
static CACHED_DEK: OnceLock<Mutex<Option<MasterKey>>> = OnceLock::new();

fn cache_slot() -> &'static Mutex<Option<MasterKey>> {
    CACHED_DEK.get_or_init(|| Mutex::new(None))
}

/// 32-byte data-encryption key (zeroized on drop).
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MasterKey {
    bytes: [u8; DEK_LEN],
}

impl MasterKey {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; DEK_LEN] {
        &self.bytes
    }

    fn random() -> Result<Self> {
        let mut bytes = [0u8; DEK_LEN];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Ok(Self { bytes })
    }

    fn from_bytes(bytes: [u8; DEK_LEN]) -> Self {
        Self { bytes }
    }
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey(***)")
    }
}

/// Path to `{files_dir}/master.key`.
#[must_use]
pub fn master_key_path(files_dir: &Path) -> PathBuf {
    files_dir.join(MASTER_KEY_FILE_NAME)
}

/// Load or mint the DEK and cache it for this process.
///
/// Call once at CLI/daemon startup after `paths.ensure_dirs()`.
pub fn configure_master_key(files_dir: &Path) -> Result<MasterKey> {
    let key = resolve_master_key(files_dir)?;
    if let Ok(mut guard) = cache_slot().lock() {
        *guard = Some(key.clone());
    }
    Ok(key)
}

/// Return the process-cached DEK, or resolve from `files_dir` if not configured.
pub fn require_master_key(files_dir: Option<&Path>) -> Result<MasterKey> {
    if let Ok(guard) = cache_slot().lock() {
        if let Some(key) = guard.as_ref() {
            return Ok(key.clone());
        }
    }
    let dir = files_dir.ok_or_else(|| {
        LibraryError::Other(anyhow::anyhow!(
            "master key not configured — call configure_master_key(files_dir) at startup"
        ))
    })?;
    configure_master_key(dir)
}

/// Resolve the DEK from env + `master.key` (minting when needed).
pub fn resolve_master_key(files_dir: &Path) -> Result<MasterKey> {
    let path = master_key_path(files_dir);
    let password = read_auth_password_env();

    if path.is_file() {
        let raw = std::fs::read(&path).map_err(|e| {
            LibraryError::Other(anyhow::anyhow!(
                "failed to read master key {}: {e}",
                path.display()
            ))
        })?;
        return parse_master_key_file(&raw, password.as_deref(), &path);
    }

    // Mint a new DEK.
    let dek = MasterKey::random()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            LibraryError::Other(anyhow::anyhow!(
                "failed to create master key directory {}: {e}",
                parent.display()
            ))
        })?;
        harden_path(parent, true)?;
    }
    let bytes = match password.as_deref() {
        Some(pw) => encode_wrapped_master_key(&dek, pw)?,
        None => encode_raw_master_key(&dek),
    };
    write_secret_file(&path, &bytes)?;
    tracing::info!(
        path = %path.display(),
        wrapped = password.is_some(),
        "minted data-encryption key for encrypted_secrets (mode 0600)"
    );
    Ok(dek)
}

fn read_auth_password_env() -> Option<String> {
    let v = std::env::var(AUTH_PASSWORD_ENV).ok()?;
    let trimmed = v.trim();
    if trimmed.is_empty() {
        return None;
    }
    bookclerk_config::register_secret(trimmed);
    Some(trimmed.to_string())
}

fn parse_master_key_file(raw: &[u8], password: Option<&str>, path: &Path) -> Result<MasterKey> {
    if raw.len() < 4 {
        return Err(LibraryError::Other(anyhow::anyhow!(
            "master key file {} is truncated",
            path.display()
        )));
    }
    match &raw[..4] {
        b"BCK1" => {
            if raw.len() != 4 + DEK_LEN {
                return Err(LibraryError::Other(anyhow::anyhow!(
                    "master key file {} has invalid BCK1 length",
                    path.display()
                )));
            }
            let mut bytes = [0u8; DEK_LEN];
            bytes.copy_from_slice(&raw[4..]);
            if let Some(pw) = password {
                // Password newly set: re-wrap in place for at-rest protection.
                let dek = MasterKey::from_bytes(bytes);
                let wrapped = encode_wrapped_master_key(&dek, pw)?;
                write_secret_file(path, &wrapped)?;
                tracing::info!(
                    path = %path.display(),
                    "re-wrapped master.key with BOOKCLERK_AUTH_PASSWORD"
                );
                return Ok(dek);
            }
            Ok(MasterKey::from_bytes(bytes))
        }
        b"BCK2" => {
            let pw = password.ok_or_else(|| {
                LibraryError::Other(anyhow::anyhow!(
                    "master key file {} is password-wrapped — set {AUTH_PASSWORD_ENV}",
                    path.display()
                ))
            })?;
            decode_wrapped_master_key(&raw[4..], pw, path)
        }
        _ => Err(LibraryError::Other(anyhow::anyhow!(
            "master key file {} has unknown magic (expected BCK1/BCK2)",
            path.display()
        ))),
    }
}

fn encode_raw_master_key(dek: &MasterKey) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + DEK_LEN);
    out.extend_from_slice(MAGIC_RAW);
    out.extend_from_slice(dek.as_bytes());
    out
}

fn encode_wrapped_master_key(dek: &MasterKey, password: &str) -> Result<Vec<u8>> {
    let salt = random_bytes(SALT_LEN);
    let nonce = random_bytes(NONCE_LEN);
    let key = derive_wrapping_key(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| LibraryError::Other(anyhow::anyhow!("cipher init: {e}")))?;
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), dek.as_bytes().as_slice())
        .map_err(|_| LibraryError::Other(anyhow::anyhow!("failed to wrap master key")))?;
    let mut out = Vec::with_capacity(4 + SALT_LEN + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC_WRAPPED);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    let _ = (
        KDF_ALGORITHM,
        CIPHER_ALGORITHM,
        KDF_M_COST,
        KDF_T_COST,
        KDF_P_COST,
    );
    Ok(out)
}

fn decode_wrapped_master_key(body: &[u8], password: &str, path: &Path) -> Result<MasterKey> {
    if body.len() < SALT_LEN + NONCE_LEN + 16 {
        return Err(LibraryError::Other(anyhow::anyhow!(
            "master key file {} wrapped payload truncated",
            path.display()
        )));
    }
    let salt = &body[..SALT_LEN];
    let nonce = &body[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ct = &body[SALT_LEN + NONCE_LEN..];
    let key = derive_wrapping_key(password, salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| LibraryError::Other(anyhow::anyhow!("cipher init: {e}")))?;
    let plain = cipher.decrypt(XNonce::from_slice(nonce), ct).map_err(|_| {
        LibraryError::Other(anyhow::anyhow!(
            "failed to unwrap master key {} — wrong {AUTH_PASSWORD_ENV}?",
            path.display()
        ))
    })?;
    if plain.len() != DEK_LEN {
        return Err(LibraryError::Other(anyhow::anyhow!(
            "unwrapped master key has invalid length"
        )));
    }
    let mut bytes = [0u8; DEK_LEN];
    bytes.copy_from_slice(&plain);
    Ok(MasterKey::from_bytes(bytes))
}

fn derive_wrapping_key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let params = ArgonParams::new(KDF_M_COST, KDF_T_COST, KDF_P_COST, Some(32))
        .map_err(|e| LibraryError::Other(anyhow::anyhow!("argon2 params: {e}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| LibraryError::Other(anyhow::anyhow!("argon2 hash: {e}")))?;
    Ok(key)
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    rand::rngs::OsRng.fill_bytes(&mut out);
    out
}

fn write_secret_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).map_err(|e| {
        LibraryError::Other(anyhow::anyhow!(
            "failed to write master key {}: {e}",
            path.display()
        ))
    })?;
    file.write_all(bytes).map_err(|e| {
        LibraryError::Other(anyhow::anyhow!(
            "failed to write master key {}: {e}",
            path.display()
        ))
    })?;
    harden_path(path, false)?;
    Ok(())
}

fn harden_path(path: &Path, is_dir: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if is_dir { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|e| {
            LibraryError::Other(anyhow::anyhow!("failed to harden {}: {e}", path.display()))
        })?;
    }
    let _ = (path, is_dir);
    Ok(())
}

/// Seal plaintext with the DEK (random nonce). Fast — no Argon2.
pub fn seal_with_dek(plaintext: &[u8], dek: &MasterKey) -> Result<(Vec<u8>, Vec<u8>)> {
    let nonce = random_bytes(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new_from_slice(dek.as_bytes())
        .map_err(|e| LibraryError::Other(anyhow::anyhow!("cipher init: {e}")))?;
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|_| LibraryError::Other(anyhow::anyhow!("seal failed")))?;
    Ok((ciphertext, nonce))
}

/// Unseal ciphertext with the DEK.
pub fn unseal_with_dek(ciphertext: &[u8], nonce: &[u8], dek: &MasterKey) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(dek.as_bytes())
        .map_err(|e| LibraryError::Other(anyhow::anyhow!("cipher init: {e}")))?;
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| {
            LibraryError::Other(anyhow::anyhow!(
                "unseal failed — wrong master key or corrupted ciphertext"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn mint_raw_and_reload() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        std::env::remove_var(AUTH_PASSWORD_ENV);
        let a = resolve_master_key(dir.path()).unwrap();
        let b = resolve_master_key(dir.path()).unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes());
        assert!(master_key_path(dir.path()).is_file());
    }

    #[test]
    fn wrap_with_password() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        std::env::set_var(AUTH_PASSWORD_ENV, "unit-test-master-pass");
        let a = resolve_master_key(dir.path()).unwrap();
        let b = resolve_master_key(dir.path()).unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes());
        std::env::remove_var(AUTH_PASSWORD_ENV);
        assert!(resolve_master_key(dir.path()).is_err());
        std::env::set_var(AUTH_PASSWORD_ENV, "unit-test-master-pass");
        let c = resolve_master_key(dir.path()).unwrap();
        assert_eq!(a.as_bytes(), c.as_bytes());
        std::env::remove_var(AUTH_PASSWORD_ENV);
    }

    #[test]
    fn seal_roundtrip() {
        let dek = MasterKey::random().unwrap();
        let (ct, nonce) = seal_with_dek(b"hello-secret", &dek).unwrap();
        let plain = unseal_with_dek(&ct, &nonce, &dek).unwrap();
        assert_eq!(plain, b"hello-secret");
    }
}
