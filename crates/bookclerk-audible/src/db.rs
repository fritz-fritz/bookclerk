//! DB-backed credential storage for Audible accounts.
//!
//! Replaces `Accounts/*.audible.auth` files with rows in the `encrypted_secrets`
//! table (kind = `source_auth`, provider = `audible`).
//!
//! The audible-rs envelope (already encrypted with its own KDF / cipher) is
//! stored verbatim as `format = "audible-rs-auth"` — the envelope's own
//! Argon2id + XChaCha20-Poly1305 protection is sufficient.

use audible_rs::auth::authfile::KdfParams;
use audible_rs::auth::Authenticator;
use bookclerk_library::{
    secret_kind, upsert_secret, EncryptedSecretRecord, LibraryStore, SecretStore,
};
use chrono::Utc;

use crate::error::{AudibleError, Result};
use crate::secret::resolve_auth_password;

// ── Internal helpers ─────────────────────────────────────────────────────────

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

// ── Save ─────────────────────────────────────────────────────────────────────

/// Persist an [`Authenticator`] into the `encrypted_secrets` table.
///
/// The audible-rs envelope is serialized with its own protection (Argon2id +
/// XChaCha20-Poly1305 when a password is configured, plain when
/// `allow_plaintext` is set) and stored as `format = "audible-rs-auth"`.
///
/// `account_name` becomes both `account_id` and `name` in the row.
pub async fn save_authenticator_to_db(
    auth: &Authenticator,
    library: &LibraryStore,
    account_name: &str,
    allow_plaintext: bool,
) -> Result<()> {
    let password = resolve_auth_password()?;
    let password_ref = password.as_ref();

    if password_ref.is_none() && !allow_plaintext {
        return Err(AudibleError::Auth(format!(
            "auth encryption requires a passphrase — set {} or set \
             auth.allow_plaintext = true to store without encryption",
            crate::secret::AUTH_PASSWORD_ENV
        )));
    }

    let content = tokio::task::spawn_blocking({
        let data = auth.export_value();
        let protection = if password_ref.is_some() {
            audible_rs::auth::authfile::Protection::Encrypted(KdfParams::default())
        } else {
            audible_rs::auth::authfile::Protection::Plain
        };
        let password = password.clone();
        move || {
            audible_rs::auth::authfile::write(&data, protection, password.as_ref())
                .map_err(|e| AudibleError::Auth(e.to_string()))
        }
    })
    .await
    .expect("blocking authfile write must not panic")?;

    let now = now_rfc3339();
    let record = EncryptedSecretRecord {
        id: None,
        kind: secret_kind::SOURCE_AUTH.to_string(),
        provider: Some("audible".to_string()),
        account_id: Some(account_name.to_string()),
        name: format!("{}.audible.auth", account_name),
        format: "audible-rs-auth".to_string(),
        ciphertext: content.into_bytes(),
        kdf_algorithm: None,
        kdf_salt: None,
        kdf_m_cost: None,
        kdf_t_cost: None,
        kdf_p_cost: None,
        cipher_algorithm: None,
        cipher_nonce: None,
        created_at: now.clone(),
        updated_at: now,
    };

    upsert_secret(library.db(), &record)
        .await
        .map_err(|e| AudibleError::Auth(format!("failed to save audible auth to DB: {e}")))?;

    tracing::info!(account = %account_name, "audible auth stored in encrypted_secrets");
    Ok(())
}

// ── Load ─────────────────────────────────────────────────────────────────────

/// Load an [`Authenticator`] from the `encrypted_secrets` table.
///
/// Registers a write-back callback so that token refreshes and cookie
/// exchanges persist back to the DB automatically.
///
/// Returns `None` when no secret for the given `account_name` exists.
pub async fn load_authenticator_from_db(
    library: &LibraryStore,
    account_name: &str,
    allow_plaintext: bool,
) -> Result<Option<Authenticator>> {
    let store = SecretStore::new(library.db());
    let name = format!("{}.audible.auth", account_name);
    let record = store
        .get(
            secret_kind::SOURCE_AUTH,
            Some("audible"),
            Some(account_name),
            &name,
        )
        .await
        .map_err(|e| AudibleError::Auth(format!("DB lookup failed for {account_name}: {e}")))?;

    let Some(record) = record else {
        return Ok(None);
    };

    let raw_bytes = record.ciphertext.clone();
    let password = resolve_auth_password()?;

    let mut auth = tokio::task::spawn_blocking(move || {
        Authenticator::load_from_bytes(&raw_bytes, password)
            .map_err(|e| AudibleError::Auth(format!("failed to decode audible auth: {e}")))
    })
    .await
    .expect("blocking authfile decode must not panic")?;

    // Register async token-refresh write-back so refreshes persist to DB.
    let db_clone = library.db().clone();
    let account_name_owned = account_name.to_string();
    auth.set_write_back_fn(move |value: serde_json::Value| {
        let db_inner = db_clone.clone();
        let acct = account_name_owned.clone();
        async move {
            let password = resolve_auth_password()
                .map_err(|e| audible_rs::auth::AuthError::InvalidData(e.to_string()))?;
            let protection = if password.is_some() {
                audible_rs::auth::authfile::Protection::Encrypted(KdfParams::default())
            } else if allow_plaintext {
                audible_rs::auth::authfile::Protection::Plain
            } else {
                return Err(audible_rs::auth::AuthError::InvalidData(
                    "no password configured and plaintext not allowed".to_string(),
                ));
            };
            let content = audible_rs::auth::authfile::write(&value, protection, password.as_ref())
                .map_err(|e| audible_rs::auth::AuthError::InvalidData(e.to_string()))?;

            let now = Utc::now().to_rfc3339();
            let record = EncryptedSecretRecord {
                id: None,
                kind: secret_kind::SOURCE_AUTH.to_string(),
                provider: Some("audible".to_string()),
                account_id: Some(acct.clone()),
                name: format!("{}.audible.auth", acct),
                format: "audible-rs-auth".to_string(),
                ciphertext: content.into_bytes(),
                kdf_algorithm: None,
                kdf_salt: None,
                kdf_m_cost: None,
                kdf_t_cost: None,
                kdf_p_cost: None,
                cipher_algorithm: None,
                cipher_nonce: None,
                created_at: now.clone(),
                updated_at: now,
            };
            upsert_secret(&db_inner, &record)
                .await
                .map_err(|e| audible_rs::auth::AuthError::InvalidData(e.to_string()))?;
            tracing::debug!(account = %acct, "audible token refreshed → encrypted_secrets");
            Ok(())
        }
    });

    register_authenticator_secrets(&auth);
    Ok(Some(auth))
}

// ── List ─────────────────────────────────────────────────────────────────────

/// List all Audible accounts stored in the DB.
///
/// Returns `(account_id, name)` tuples extracted from `encrypted_secrets` rows.
pub async fn list_audible_accounts_from_db(
    library: &LibraryStore,
) -> Result<Vec<(String, String)>> {
    let store = SecretStore::new(library.db());
    let records = store
        .list(secret_kind::SOURCE_AUTH)
        .await
        .map_err(|e| AudibleError::Auth(format!("DB list failed: {e}")))?;
    Ok(records
        .into_iter()
        .filter(|r| r.provider.as_deref() == Some("audible"))
        .filter_map(|r| {
            let account_id = r.account_id?;
            Some((account_id, r.name))
        })
        .collect())
}

// ── Delete ───────────────────────────────────────────────────────────────────

/// Remove an Audible account secret from the DB.
pub async fn delete_audible_account_from_db(
    library: &LibraryStore,
    account_name: &str,
) -> Result<()> {
    let store = SecretStore::new(library.db());
    let name = format!("{}.audible.auth", account_name);
    store
        .delete(
            secret_kind::SOURCE_AUTH,
            Some("audible"),
            Some(account_name),
            &name,
        )
        .await
        .map_err(|e| {
            AudibleError::Auth(format!(
                "failed to delete audible auth from DB for {account_name}: {e}"
            ))
        })
}

// ── Widevine CDM ─────────────────────────────────────────────────────────────

/// Persist a raw Widevine `.wvd` device blob into `encrypted_secrets`.
///
/// `account_id` identifies which account the CDM was provisioned for.
/// The blob is stored verbatim — its own protection comes from the Widevine
/// L3 provisioning flow (not from Bookclerk-level encryption).
pub async fn save_widevine_cdm_to_db(
    library: &LibraryStore,
    account_id: &str,
    wvd_bytes: &[u8],
) -> Result<()> {
    let now = now_rfc3339();
    let name = format!("{}.wvd", account_id);
    let record = EncryptedSecretRecord {
        id: None,
        kind: secret_kind::WIDEVINE.to_string(),
        provider: Some("audible".to_string()),
        account_id: Some(account_id.to_string()),
        name,
        format: "wvd".to_string(),
        ciphertext: wvd_bytes.to_vec(),
        kdf_algorithm: None,
        kdf_salt: None,
        kdf_m_cost: None,
        kdf_t_cost: None,
        kdf_p_cost: None,
        cipher_algorithm: None,
        cipher_nonce: None,
        created_at: now.clone(),
        updated_at: now,
    };
    upsert_secret(library.db(), &record)
        .await
        .map_err(|e| AudibleError::Widevine(format!("failed to save Widevine CDM to DB: {e}")))?;
    tracing::info!(account = %account_id, "Widevine CDM stored in encrypted_secrets");
    Ok(())
}

/// Load a Widevine `.wvd` device blob from `encrypted_secrets`.
///
/// Returns `None` when no CDM for `account_id` is stored yet.
pub async fn load_widevine_cdm_from_db(
    library: &LibraryStore,
    account_id: &str,
) -> Result<Option<Vec<u8>>> {
    let store = SecretStore::new(library.db());
    let name = format!("{}.wvd", account_id);
    let record = store
        .get(
            secret_kind::WIDEVINE,
            Some("audible"),
            Some(account_id),
            &name,
        )
        .await
        .map_err(|e| {
            AudibleError::Widevine(format!(
                "DB lookup failed for Widevine CDM {account_id}: {e}"
            ))
        })?;
    Ok(record.map(|r| r.ciphertext))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn register_authenticator_secrets(auth: &Authenticator) {
    use secrecy::ExposeSecret;
    if let Some(t) = auth.access_token() {
        bookclerk_config::register_secret(t.expose_secret());
    }
    if let Some(t) = auth.refresh_token() {
        bookclerk_config::register_secret(t.expose_secret());
    }
}
