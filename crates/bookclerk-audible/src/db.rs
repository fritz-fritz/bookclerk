//! DB-backed credential storage for Audible accounts.
//!
//! Replaces `Accounts/*.audible.auth` files with rows in the `encrypted_secrets`
//! table (kind = `source_auth`, provider = `audible`).
//!
//! Each credential is stored as `format = "sealed-v1"` wrapping a
//! `Protection::Plain` audible-rs envelope. The outer XChaCha20-Poly1305 seal
//! (process DEK from `master.key`) provides at-rest protection; audible-rs
//! inner encryption is intentionally bypassed so key derivation happens once
//! at startup rather than on every credential access.

use audible_rs::auth::Authenticator;
use bookclerk_library::{
    build_sealed_record, secret_account_type, secret_kind, unseal_secret, upsert_secret,
    EncryptedSecretRecord, LibraryStore, SecretStore, FORMAT_SEALED_V1,
};

use crate::error::{AudibleError, Result};

// ── Internal helpers ─────────────────────────────────────────────────────────

fn audible_name(account_name: &str) -> String {
    format!("{account_name}.audible.auth")
}

// ── Save ─────────────────────────────────────────────────────────────────────

/// Persist an [`Authenticator`] into the `encrypted_secrets` table.
///
/// The audible-rs envelope is serialized with `Protection::Plain` (no inner
/// Argon2), then sealed with the process DEK (`sealed-v1`).
pub async fn save_authenticator_to_db(
    auth: &Authenticator,
    library: &LibraryStore,
    account_name: &str,
) -> Result<()> {
    let plain_bytes = tokio::task::spawn_blocking({
        let data = auth.export_value();
        move || {
            audible_rs::auth::authfile::write(
                &data,
                audible_rs::auth::authfile::Protection::Plain,
                None,
            )
            .map_err(|e| AudibleError::Auth(e.to_string()))
        }
    })
    .await
    .expect("blocking authfile write must not panic")?;

    let name = audible_name(account_name);
    let record = build_sealed_record(
        plain_bytes.as_bytes(),
        secret_kind::SOURCE_AUTH,
        "audible",
        secret_account_type::INTEGRATION,
        account_name,
        &name,
    )
    .map_err(|e| AudibleError::Auth(format!("failed to seal audible auth: {e}")))?;

    upsert_secret(library.db(), &record)
        .await
        .map_err(|e| AudibleError::Auth(format!("failed to save audible auth to DB: {e}")))?;

    tracing::info!(account = %account_name, "audible auth stored in encrypted_secrets (sealed-v1)");
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
) -> Result<Option<Authenticator>> {
    let store = SecretStore::new(library.db());
    let name = audible_name(account_name);
    let record = store
        .get(
            secret_kind::SOURCE_AUTH,
            Some("audible"),
            secret_account_type::INTEGRATION,
            Some(account_name),
            &name,
        )
        .await
        .map_err(|e| AudibleError::Auth(format!("DB lookup failed for {account_name}: {e}")))?;

    let Some(record) = record else {
        return Ok(None);
    };

    let plain_bytes = unseal_record_for_audible(&record, account_name)?;

    let mut auth = tokio::task::spawn_blocking(move || {
        Authenticator::load_from_bytes(&plain_bytes, None)
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
            let plain_bytes = audible_rs::auth::authfile::write(
                &value,
                audible_rs::auth::authfile::Protection::Plain,
                None,
            )
            .map_err(|e| audible_rs::auth::AuthError::InvalidData(e.to_string()))?;

            let name = audible_name(&acct);
            let record = build_sealed_record(
                plain_bytes.as_bytes(),
                secret_kind::SOURCE_AUTH,
                "audible",
                secret_account_type::INTEGRATION,
                &acct,
                &name,
            )
            .map_err(|e| audible_rs::auth::AuthError::InvalidData(e.to_string()))?;

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

/// Unseal an audible auth record (sealed-v1 or legacy formats).
fn unseal_record_for_audible(
    record: &EncryptedSecretRecord,
    account_name: &str,
) -> Result<Vec<u8>> {
    match record.format.as_str() {
        FORMAT_SEALED_V1 => unseal_secret(record).map_err(|e| {
            AudibleError::Auth(format!(
                "failed to unseal audible auth for {account_name}: {e}"
            ))
        }),
        "audible-rs-auth" => {
            // Legacy: audible-rs envelope with its own inner encryption.
            // Loaded raw; audible-rs will decrypt using BOOKCLERK_AUTH_PASSWORD if needed.
            Ok(record.ciphertext.clone())
        }
        other => Err(AudibleError::Auth(format!(
            "unsupported audible auth format {other:?} for account {account_name}"
        ))),
    }
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
    let name = audible_name(account_name);
    store
        .delete(
            secret_kind::SOURCE_AUTH,
            Some("audible"),
            secret_account_type::INTEGRATION,
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
/// The blob is sealed with the process DEK (`sealed-v1`).
pub async fn save_widevine_cdm_to_db(
    library: &LibraryStore,
    account_id: &str,
    wvd_bytes: &[u8],
) -> Result<()> {
    let name = format!("{account_id}.wvd");
    let record = build_sealed_record(
        wvd_bytes,
        secret_kind::WIDEVINE,
        "audible",
        secret_account_type::INTEGRATION,
        account_id,
        &name,
    )
    .map_err(|e| AudibleError::Widevine(format!("failed to seal Widevine CDM: {e}")))?;

    upsert_secret(library.db(), &record)
        .await
        .map_err(|e| AudibleError::Widevine(format!("failed to save Widevine CDM to DB: {e}")))?;
    tracing::info!(account = %account_id, "Widevine CDM stored in encrypted_secrets (sealed-v1)");
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
    let name = format!("{account_id}.wvd");
    let record = store
        .get(
            secret_kind::WIDEVINE,
            Some("audible"),
            secret_account_type::INTEGRATION,
            Some(account_id),
            &name,
        )
        .await
        .map_err(|e| {
            AudibleError::Widevine(format!(
                "DB lookup failed for Widevine CDM {account_id}: {e}"
            ))
        })?;

    let Some(record) = record else {
        return Ok(None);
    };

    match record.format.as_str() {
        FORMAT_SEALED_V1 => {
            let bytes = unseal_secret(&record).map_err(|e| {
                AudibleError::Widevine(format!(
                    "failed to unseal Widevine CDM for {account_id}: {e}"
                ))
            })?;
            Ok(Some(bytes))
        }
        "wvd" => {
            // Legacy: raw WVD bytes without outer encryption.
            Ok(Some(record.ciphertext))
        }
        other => Err(AudibleError::Widevine(format!(
            "unsupported Widevine CDM format {other:?} for account {account_id}"
        ))),
    }
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
