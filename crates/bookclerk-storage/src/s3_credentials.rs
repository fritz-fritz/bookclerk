//! S3 destination credentials in `encrypted_secrets`.
//!
//! Payload JSON is stored as `kind = "s3"`, `name = "default"`. Prefer
//! encrypting with `BOOKCLERK_AUTH_PASSWORD` (`format = "json-encrypted"`).

use bookclerk_library::{
    decrypt_secret, delete_secret, encrypt_secret, secret_kind, upsert_secret,
    EncryptedSecretRecord, CIPHER_ALGORITHM, KDF_ALGORITHM, KDF_M_COST, KDF_P_COST, KDF_T_COST,
};
use chrono::Utc;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

use crate::error::{Result, StorageError};

/// Canonical secret name for the default S3 destination credentials.
pub const S3_SECRET_NAME: &str = "default";

/// Static AWS-style credentials for the S3 destination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct S3Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn map_lib(err: bookclerk_library::LibraryError) -> StorageError {
    StorageError::S3(format!("encrypted_secrets: {err}"))
}

/// Persist S3 credentials into `encrypted_secrets`.
///
/// When `password` is set, encrypts with Argon2id + XChaCha20-Poly1305.
pub async fn save_s3_credentials(
    db: &DatabaseConnection,
    creds: &S3Credentials,
    password: Option<&str>,
) -> Result<()> {
    if creds.access_key_id.trim().is_empty() || creds.secret_access_key.trim().is_empty() {
        return Err(StorageError::S3(
            "S3 access_key_id and secret_access_key must not be empty".into(),
        ));
    }
    bookclerk_config::register_secret(&creds.access_key_id);
    bookclerk_config::register_secret(&creds.secret_access_key);
    if let Some(token) = &creds.session_token {
        bookclerk_config::register_secret(token);
    }

    let json = serde_json::to_vec(creds)
        .map_err(|e| StorageError::S3(format!("failed to serialize S3 credentials: {e}")))?;
    let now = now_rfc3339();
    let record = if let Some(pwd) = password.filter(|p| !p.is_empty()) {
        let blob = encrypt_secret(&json, pwd).map_err(map_lib)?;
        EncryptedSecretRecord {
            id: None,
            kind: secret_kind::S3.to_string(),
            provider: Some("s3".to_string()),
            account_id: None,
            name: S3_SECRET_NAME.to_string(),
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
        tracing::warn!("storing S3 credentials without encryption (no BOOKCLERK_AUTH_PASSWORD)");
        EncryptedSecretRecord {
            id: None,
            kind: secret_kind::S3.to_string(),
            provider: Some("s3".to_string()),
            account_id: None,
            name: S3_SECRET_NAME.to_string(),
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
    upsert_secret(db, &record).await.map_err(map_lib)?;
    Ok(())
}

/// Load S3 credentials from `encrypted_secrets`, if present.
pub async fn load_s3_credentials(
    db: &DatabaseConnection,
    password: Option<&str>,
) -> Result<Option<S3Credentials>> {
    let store = bookclerk_library::SecretStore::new(db);
    let Some(record) = store
        .get(secret_kind::S3, Some("s3"), None, S3_SECRET_NAME)
        .await
        .map_err(map_lib)?
    else {
        return Ok(None);
    };

    let json = match record.format.as_str() {
        "json-encrypted" => {
            let pwd = password.ok_or_else(|| {
                StorageError::S3(
                    "S3 credentials in encrypted_secrets require BOOKCLERK_AUTH_PASSWORD \
                     (or BOOKCLERK_AUTH_PASSWORD_FILE / [auth].password_file)"
                        .into(),
                )
            })?;
            let salt = record
                .kdf_salt
                .as_deref()
                .ok_or_else(|| StorageError::S3("S3 secret missing kdf_salt".into()))?;
            let nonce = record
                .cipher_nonce
                .as_deref()
                .ok_or_else(|| StorageError::S3("S3 secret missing cipher_nonce".into()))?;
            decrypt_secret(&record.ciphertext, pwd, salt, nonce).map_err(map_lib)?
        }
        "json" => record.ciphertext,
        other => {
            return Err(StorageError::S3(format!(
                "unsupported S3 secret format `{other}`"
            )));
        }
    };

    let creds: S3Credentials = serde_json::from_slice(&json)
        .map_err(|e| StorageError::S3(format!("invalid S3 credentials JSON: {e}")))?;
    bookclerk_config::register_secret(&creds.access_key_id);
    bookclerk_config::register_secret(&creds.secret_access_key);
    if let Some(token) = &creds.session_token {
        bookclerk_config::register_secret(token);
    }
    Ok(Some(creds))
}

/// Delete stored S3 credentials from `encrypted_secrets`.
pub async fn delete_s3_credentials(db: &DatabaseConnection) -> Result<()> {
    delete_secret(db, secret_kind::S3, Some("s3"), None, S3_SECRET_NAME)
        .await
        .map_err(map_lib)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookclerk_library::connect_sqlite_memory;

    #[tokio::test]
    async fn roundtrip_encrypted() {
        let db = connect_sqlite_memory().await.unwrap();
        let creds = S3Credentials {
            access_key_id: "AKIAEXAMPLE".into(),
            secret_access_key: "secret".into(),
            session_token: Some("tok".into()),
            label: Some("minio".into()),
        };
        save_s3_credentials(&db, &creds, Some("unit-test-pass"))
            .await
            .unwrap();
        let loaded = load_s3_credentials(&db, Some("unit-test-pass"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded, creds);
        delete_s3_credentials(&db).await.unwrap();
        assert!(load_s3_credentials(&db, Some("unit-test-pass"))
            .await
            .unwrap()
            .is_none());
    }
}
