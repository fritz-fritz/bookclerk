//! Canonical object encoding, SHA-256 identity, and independent compression.
//!
//! Content addresses are SHA-256 of the **uncompressed** canonical JSON. Stored
//! objects may be gzip-compressed (`flate2`, already in this crate) so identical
//! logical content keeps the same digest regardless of compression settings.

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};

use bookclerk_plugin_abi::DbValue;

use super::IdentityHighWater;
use crate::error::{LibraryError, Result};

/// Magic prefix for stored backup objects (`BCKO`).
pub const OBJECT_MAGIC: &[u8; 4] = b"BCKO";
/// Stored object envelope version.
pub const OBJECT_STORE_VERSION: u8 = 1;
/// Stored payload is uncompressed canonical JSON.
pub const CODEC_RAW: u8 = 0;
/// Stored payload is gzip of canonical JSON.
pub const CODEC_GZIP: u8 = 1;

/// Target uncompressed JSON size for one table-data chunk.
///
/// 256 KiB sits in a 128–512 KiB band: large enough to avoid pathological
/// per-object overhead on typical catalog rows, small enough for a small-VPS
/// working set and for `maxResultBytes` paging. A single row larger than the
/// target still occupies its own chunk.
pub const CHUNK_TARGET_UNCOMPRESSED_BYTES: usize = 256 * 1024;

/// One immutable canonical object (hashed before compression).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CanonicalObject {
    /// Admitted `CREATE TABLE` / `CREATE INDEX` in restore order.
    Schema {
        /// Bookclerk SQL grammar/ABI contract.
        sql_contract_version: u32,
        /// Canonical statements (tables in FK-safe order, then indexes).
        statements: Vec<String>,
    },
    /// Bounded table rows in canonical order.
    TableChunk {
        /// Folded table name.
        table: String,
        /// Column names in CREATE declaration order.
        columns: Vec<String>,
        /// Row-major `DbValue` cells.
        rows: Vec<Vec<DbValue>>,
    },
    /// Identity high-water for generated columns.
    Identity {
        /// Folded table name → high-water.
        entries: BTreeMap<String, IdentityHighWater>,
    },
}

/// SHA-256 hex of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Deterministic JSON encoding of a canonical object (uncompressed identity).
///
/// # Errors
///
/// Returns when JSON serialization fails.
pub fn encode_canonical_object(object: &CanonicalObject) -> Result<Vec<u8>> {
    serde_json::to_vec(object)
        .map_err(|err| LibraryError::Other(anyhow::anyhow!("canonical backup object json: {err}")))
}

/// Parses canonical object JSON.
///
/// # Errors
///
/// Returns when the bytes are not a supported [`CanonicalObject`].
pub fn decode_canonical_object(bytes: &[u8]) -> Result<CanonicalObject> {
    serde_json::from_slice(bytes)
        .map_err(|err| LibraryError::Schema(format!("canonical backup object json: {err}")))
}

/// Encodes an on-disk object: magic, version, gzip codec, digest, payload.
///
/// # Errors
///
/// Returns when gzip compression fails.
pub fn wrap_stored_object(uncompressed: &[u8]) -> Result<Vec<u8>> {
    let digest = Sha256::digest(uncompressed);
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(uncompressed)
        .map_err(|err| LibraryError::Other(anyhow::anyhow!("backup object gzip: {err}")))?;
    let payload = gz
        .finish()
        .map_err(|err| LibraryError::Other(anyhow::anyhow!("backup object gzip finish: {err}")))?;
    let mut out = Vec::with_capacity(4 + 1 + 1 + 2 + 32 + payload.len());
    out.extend_from_slice(OBJECT_MAGIC);
    out.push(OBJECT_STORE_VERSION);
    out.push(CODEC_GZIP);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&digest);
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Unwraps a stored object, verifying the embedded uncompressed digest.
///
/// # Errors
///
/// Returns when the envelope is truncated, the codec is unknown, gzip fails,
/// or the uncompressed SHA-256 does not match the embedded digest.
pub fn unwrap_stored_object(stored: &[u8]) -> Result<Vec<u8>> {
    if stored.len() < 40 {
        return Err(LibraryError::Schema(
            "backup object is truncated or corrupt".into(),
        ));
    }
    if stored[..4] != *OBJECT_MAGIC {
        return Err(LibraryError::Schema(
            "backup object magic is not BCKO".into(),
        ));
    }
    if stored[4] != OBJECT_STORE_VERSION {
        return Err(LibraryError::Schema(format!(
            "unsupported backup object store version {}",
            stored[4]
        )));
    }
    let codec = stored[5];
    let embedded: [u8; 32] = stored[8..40]
        .try_into()
        .map_err(|_| LibraryError::Schema("backup object digest is truncated".into()))?;
    let payload = &stored[40..];
    let uncompressed = match codec {
        CODEC_RAW => payload.to_vec(),
        CODEC_GZIP => {
            let mut dec = GzDecoder::new(payload);
            let mut out = Vec::new();
            dec.read_to_end(&mut out).map_err(|err| {
                LibraryError::Schema(format!("backup object gzip decode failed: {err}"))
            })?;
            out
        }
        other => {
            return Err(LibraryError::Schema(format!(
                "unknown backup object codec {other}"
            )));
        }
    };
    let actual = Sha256::digest(&uncompressed);
    if actual.as_slice() != embedded {
        return Err(LibraryError::Schema(
            "backup object uncompressed digest does not match the stored digest".into(),
        ));
    }
    Ok(uncompressed)
}

/// True when adding `row` to `chunk` would exceed `target` and the chunk
/// already has at least one row.
#[must_use]
pub fn chunk_would_overflow(chunk_bytes: usize, row: &[DbValue], target: usize) -> bool {
    if chunk_bytes == 0 {
        return false;
    }
    let row_bytes = serde_json::to_vec(row).map(|b| b.len()).unwrap_or(0);
    chunk_bytes.saturating_add(row_bytes).saturating_add(2) > target.max(1)
}
