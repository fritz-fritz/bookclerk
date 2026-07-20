use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Metadata attached to a stored object.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    /// Free-form ASIN / title tags for S3 object metadata.
    pub asin: Option<String>,
    pub title: Option<String>,
}

/// Listing entry returned by [`StorageBackend::list`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
}

/// Pluggable storage for liberated audio and sidecar files.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Backend name for logs (`local`, `s3`).
    fn name(&self) -> &'static str;

    /// Write bytes under `key`.
    async fn put(&self, key: &str, data: Bytes, meta: ObjectMeta) -> Result<()>;

    /// Read the full object.
    async fn get(&self, key: &str) -> Result<Bytes>;

    /// True when the object exists.
    async fn exists(&self, key: &str) -> Result<bool>;

    /// List objects under `prefix`.
    async fn list(&self, prefix: &str) -> Result<Vec<ObjectInfo>>;

    /// Delete an object (no-op if missing).
    async fn delete(&self, key: &str) -> Result<()>;
}
