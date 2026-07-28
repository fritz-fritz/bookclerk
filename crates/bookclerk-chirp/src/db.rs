//! DB-backed credential storage for Chirp accounts.
//!
//! Replaces `Accounts/*.chirp.auth` files with rows in the `encrypted_secrets`
//! table (kind = `source_auth`, provider = `chirp`).
//!
//! New writes use `sealed-v1` (process DEK via `master.key`). Legacy
//! `json-encrypted` rows are still readable via `BOOKCLERK_AUTH_PASSWORD`.
//! Legacy `json` plaintext rows are rejected.

use bookclerk_library::{
    build_sealed_record, secret_account_type, secret_kind, unseal_secret, upsert_secret,
    LibraryStore, SecretStore,
};

use crate::auth::ChirpAuthFile;
use crate::error::{ChirpError, Result};

fn auth_name(account_id: &str) -> String {
    format!("{}.chirp.auth", account_id)
}

/// Persist a [`ChirpAuthFile`] into the `encrypted_secrets` table using sealed-v1.
pub async fn save_auth_to_db(
    auth: &ChirpAuthFile,
    library: &LibraryStore,
    account_id: &str,
) -> Result<()> {
    let json = serde_json::to_vec(auth)
        .map_err(|e| ChirpError::auth(format!("failed to serialize Chirp auth: {e}")))?;

    let record = build_sealed_record(
        &json,
        secret_kind::SOURCE_AUTH,
        "chirp",
        secret_account_type::INTEGRATION,
        account_id,
        &auth_name(account_id),
    )
    .map_err(|e| ChirpError::auth(format!("failed to seal Chirp auth: {e}")))?;

    upsert_secret(library.db(), &record)
        .await
        .map_err(|e| ChirpError::auth(format!("failed to save Chirp auth to DB: {e}")))?;
    tracing::info!(account = %account_id, "Chirp auth stored in encrypted_secrets (sealed-v1)");
    Ok(())
}

/// Load a [`ChirpAuthFile`] from the `encrypted_secrets` table.
///
/// Handles `sealed-v1` (DEK) and legacy `json-encrypted` (BOOKCLERK_AUTH_PASSWORD).
pub async fn load_auth_from_db(
    library: &LibraryStore,
    account_id: &str,
) -> Result<Option<ChirpAuthFile>> {
    let store = SecretStore::new(library.db());
    let record = store
        .get(
            secret_kind::SOURCE_AUTH,
            Some("chirp"),
            secret_account_type::INTEGRATION,
            Some(account_id),
            &auth_name(account_id),
        )
        .await
        .map_err(|e| ChirpError::auth(format!("DB lookup failed for {account_id}: {e}")))?;

    let Some(record) = record else {
        return Ok(None);
    };

    let plaintext = unseal_secret(&record).map_err(|e| {
        ChirpError::auth(format!("failed to unseal Chirp auth for {account_id}: {e}"))
    })?;

    let auth: ChirpAuthFile = serde_json::from_slice(&plaintext).map_err(|e| {
        ChirpError::auth(format!("failed to parse Chirp auth for {account_id}: {e}"))
    })?;
    Ok(Some(auth))
}

/// List all Chirp accounts stored in the DB.
///
/// All formats are decrypted. Records that cannot be decrypted are logged and skipped.
pub async fn list_auth_from_db(library: &LibraryStore) -> Result<Vec<(String, ChirpAuthFile)>> {
    let store = SecretStore::new(library.db());
    let records = store
        .list(secret_kind::SOURCE_AUTH)
        .await
        .map_err(|e| ChirpError::auth(format!("DB list failed: {e}")))?;

    let mut out = Vec::new();
    for record in records
        .into_iter()
        .filter(|r| r.provider.as_deref() == Some("chirp"))
    {
        let Some(account_id) = record.account_id.clone() else {
            continue;
        };
        match unseal_secret(&record) {
            Ok(plaintext) => {
                if let Ok(auth) = serde_json::from_slice::<ChirpAuthFile>(&plaintext) {
                    out.push((account_id, auth));
                } else {
                    tracing::warn!(
                        account = %account_id,
                        "Chirp auth unsealed but JSON parse failed — skipping"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    account = %account_id,
                    err = %e,
                    "failed to unseal Chirp auth in list — skipping"
                );
            }
        }
    }
    Ok(out)
}

/// Remove a Chirp account secret from the DB.
pub async fn delete_auth_from_db(library: &LibraryStore, account_id: &str) -> Result<()> {
    let store = SecretStore::new(library.db());
    store
        .delete(
            secret_kind::SOURCE_AUTH,
            Some("chirp"),
            secret_account_type::INTEGRATION,
            Some(account_id),
            &auth_name(account_id),
        )
        .await
        .map_err(|e| {
            ChirpError::auth(format!(
                "failed to delete Chirp auth from DB for {account_id}: {e}"
            ))
        })
}
