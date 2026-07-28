//! DB-backed credential storage for GraphicAudio accounts.
//!
//! Replaces `Accounts/*.ga.auth` files with rows in the `encrypted_secrets`
//! table (kind = `source_auth`, provider = `graphicaudio`).

use bookclerk_library::{
    decrypt_secret, encrypt_secret, secret_kind, upsert_secret, EncryptedSecretRecord,
    LibraryStore, SecretStore, CIPHER_ALGORITHM, KDF_ALGORITHM, KDF_M_COST, KDF_P_COST, KDF_T_COST,
};
use chrono::Utc;

use crate::auth::GraphicAudioAuthFile;
use crate::error::{GraphicAudioError, Result};

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn auth_name(account_id: &str) -> String {
    format!("{}.ga.auth", account_id)
}

/// Persist a [`GraphicAudioAuthFile`] into the `encrypted_secrets` table.
pub async fn save_auth_to_db(
    auth: &GraphicAudioAuthFile,
    library: &LibraryStore,
    account_id: &str,
    password: Option<&str>,
) -> Result<()> {
    let json = serde_json::to_vec(auth).map_err(|e| {
        GraphicAudioError::Auth(format!("failed to serialize GraphicAudio auth: {e}"))
    })?;

    let now = now_rfc3339();
    let record = if let Some(pwd) = password {
        let blob = encrypt_secret(&json, pwd).map_err(|e| {
            GraphicAudioError::Auth(format!("failed to encrypt GraphicAudio auth: {e}"))
        })?;
        EncryptedSecretRecord {
            id: None,
            kind: secret_kind::SOURCE_AUTH.to_string(),
            provider: Some("graphicaudio".to_string()),
            account_id: Some(account_id.to_string()),
            name: auth_name(account_id),
            format: "json-encrypted".to_string(),
            ciphertext: blob.ciphertext,
            kdf_algorithm: Some(KDF_ALGORITHM.to_string()),
            kdf_salt: Some(blob.kdf_salt),
            kdf_m_cost: Some(KDF_M_COST),
            kdf_t_cost: Some(KDF_T_COST),
            kdf_p_cost: Some(KDF_P_COST),
            cipher_algorithm: Some(CIPHER_ALGORITHM.to_string()),
            cipher_nonce: Some(blob.cipher_nonce),
            created_at: now.clone(),
            updated_at: now,
        }
    } else {
        tracing::warn!(
            account = %account_id,
            "storing GraphicAudio auth without encryption (no BOOKCLERK_AUTH_PASSWORD)"
        );
        EncryptedSecretRecord {
            id: None,
            kind: secret_kind::SOURCE_AUTH.to_string(),
            provider: Some("graphicaudio".to_string()),
            account_id: Some(account_id.to_string()),
            name: auth_name(account_id),
            format: "json".to_string(),
            ciphertext: json,
            kdf_algorithm: None,
            kdf_salt: None,
            kdf_m_cost: None,
            kdf_t_cost: None,
            kdf_p_cost: None,
            cipher_algorithm: None,
            cipher_nonce: None,
            created_at: now.clone(),
            updated_at: now,
        }
    };

    upsert_secret(library.db(), &record).await.map_err(|e| {
        GraphicAudioError::Auth(format!("failed to save GraphicAudio auth to DB: {e}"))
    })?;
    tracing::info!(account = %account_id, "GraphicAudio auth stored in encrypted_secrets");
    Ok(())
}

/// Load a [`GraphicAudioAuthFile`] from the `encrypted_secrets` table.
pub async fn load_auth_from_db(
    library: &LibraryStore,
    account_id: &str,
    password: Option<&str>,
) -> Result<Option<GraphicAudioAuthFile>> {
    let store = SecretStore::new(library.db());
    let record = store
        .get(
            secret_kind::SOURCE_AUTH,
            Some("graphicaudio"),
            Some(account_id),
            &auth_name(account_id),
        )
        .await
        .map_err(|e| GraphicAudioError::Auth(format!("DB lookup failed for {account_id}: {e}")))?;

    let Some(record) = record else {
        return Ok(None);
    };

    let plaintext = match record.format.as_str() {
        "json" => record.ciphertext,
        "json-encrypted" => {
            let pwd = password.ok_or_else(|| {
                GraphicAudioError::Auth(format!(
                    "GraphicAudio auth for {account_id} is encrypted — set BOOKCLERK_AUTH_PASSWORD"
                ))
            })?;
            let salt = record.kdf_salt.as_deref().ok_or_else(|| {
                GraphicAudioError::Auth(format!("missing KDF salt for {account_id}"))
            })?;
            let nonce = record.cipher_nonce.as_deref().ok_or_else(|| {
                GraphicAudioError::Auth(format!("missing cipher nonce for {account_id}"))
            })?;
            decrypt_secret(&record.ciphertext, pwd, salt, nonce).map_err(|e| {
                GraphicAudioError::Auth(format!("decryption failed for {account_id}: {e}"))
            })?
        }
        other => {
            return Err(GraphicAudioError::Auth(format!(
                "unknown format {other:?} for GraphicAudio auth {account_id}"
            )))
        }
    };

    let auth: GraphicAudioAuthFile = serde_json::from_slice(&plaintext).map_err(|e| {
        GraphicAudioError::Auth(format!(
            "failed to parse GraphicAudio auth for {account_id}: {e}"
        ))
    })?;
    Ok(Some(auth))
}

/// List all GraphicAudio accounts stored in the DB.
pub async fn list_auth_from_db(
    library: &LibraryStore,
) -> Result<Vec<(String, GraphicAudioAuthFile)>> {
    let store = SecretStore::new(library.db());
    let records = store
        .list(secret_kind::SOURCE_AUTH)
        .await
        .map_err(|e| GraphicAudioError::Auth(format!("DB list failed: {e}")))?;

    let mut out = Vec::new();
    for record in records
        .into_iter()
        .filter(|r| r.provider.as_deref() == Some("graphicaudio"))
    {
        let Some(account_id) = record.account_id.clone() else {
            continue;
        };
        let plaintext = match record.format.as_str() {
            "json" => record.ciphertext.clone(),
            _ => {
                tracing::debug!(
                    account = %account_id,
                    "skipping encrypted GraphicAudio auth in list (need password to decode)"
                );
                continue;
            }
        };
        if let Ok(auth) = serde_json::from_slice::<GraphicAudioAuthFile>(&plaintext) {
            out.push((account_id, auth));
        }
    }
    Ok(out)
}

/// Remove a GraphicAudio account secret from the DB.
pub async fn delete_auth_from_db(library: &LibraryStore, account_id: &str) -> Result<()> {
    let store = SecretStore::new(library.db());
    store
        .delete(
            secret_kind::SOURCE_AUTH,
            Some("graphicaudio"),
            Some(account_id),
            &auth_name(account_id),
        )
        .await
        .map_err(|e| {
            GraphicAudioError::Auth(format!(
                "failed to delete GraphicAudio auth from DB for {account_id}: {e}"
            ))
        })
}
