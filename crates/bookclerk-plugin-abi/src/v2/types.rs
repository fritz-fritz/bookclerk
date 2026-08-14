//! Bounded DTOs for ABI v2 (never contain media bytes).

use serde::{Deserialize, Serialize};

use super::limits::ScalarLimits;

/// Guest identity returned by `BookclerkPlugin.describe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescribe {
    /// Must equal [`super::PRODUCT_API_VERSION`].
    pub api_version: u32,
    /// Plugin id matching `plugin.toml`.
    pub id: String,
    /// `source` / `integration` / `output` / `database`.
    pub kind: String,
    /// Optional UI name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Accepted RPC features (intersection with the host offer).
    #[serde(default)]
    pub rpc_features: Vec<String>,
    /// Effective numeric limits.
    pub scalar_limits: ScalarLimitsDto,
}

/// Wire form of [`ScalarLimits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalarLimitsDto {
    /// Max ordinary RPC value size in bytes.
    pub max_scalar_bytes: u32,
    /// Max `ByteSource.pull` window in bytes.
    pub max_stream_window_bytes: u32,
    /// Max objects per list page.
    pub max_list_page: u32,
}

impl From<ScalarLimits> for ScalarLimitsDto {
    fn from(value: ScalarLimits) -> Self {
        Self {
            max_scalar_bytes: value.max_scalar_bytes,
            max_stream_window_bytes: value.max_stream_window_bytes,
            max_list_page: value.max_list_page,
        }
    }
}

impl From<ScalarLimitsDto> for ScalarLimits {
    fn from(value: ScalarLimitsDto) -> Self {
        Self {
            max_scalar_bytes: value.max_scalar_bytes,
            max_stream_window_bytes: value.max_stream_window_bytes,
            max_list_page: value.max_list_page,
        }
    }
}

/// Injected destination knobs (JSON blob plus data dir).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationContext {
    /// Guest `HOME` / plugin data directory.
    pub plugin_data_dir: String,
    /// Opaque JSON (bucket, root, credentials, …).
    #[serde(default)]
    pub json: String,
}

/// Injected source knobs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceContext {
    /// Guest `HOME` / plugin data directory.
    pub plugin_data_dir: String,
    /// Opaque JSON (credentials, marketplace, …).
    #[serde(default)]
    pub json: String,
}

/// Job worker instantiation knobs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerContext {
    /// Durable job id.
    pub job_id: String,
    /// Guest `HOME` / plugin data directory.
    pub plugin_data_dir: String,
    /// Opaque JSON extras.
    #[serde(default)]
    pub json: String,
}

/// Typed domain job event (bytes never belong here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    /// Event / command type (`stream_copy`, …).
    pub event_type: String,
    /// JSON payload for that type.
    #[serde(default)]
    pub json: String,
}

/// Handler completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobOutcome {
    /// True when the handler finished successfully.
    pub ok: bool,
    /// Operator-facing summary.
    #[serde(default)]
    pub message: String,
    /// Bytes copied when this was a stream copy.
    #[serde(default)]
    pub bytes_copied: u64,
}

/// Object listing entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    /// Object key.
    pub key: String,
    /// Size in bytes.
    pub size: u64,
}

/// Metadata without a body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectMetadata {
    /// Object key.
    pub key: String,
    /// Size in bytes.
    pub size: u64,
    /// MIME type when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Opaque etag when the backend provides one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// SHA-256 digest when computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<Vec<u8>>,
}

/// Paginated list request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOptions {
    /// Key prefix filter.
    #[serde(default)]
    pub prefix: String,
    /// Opaque continuation token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Requested page size (0 = default).
    #[serde(default)]
    pub limit: u32,
}

/// One page of keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPage {
    /// Objects in this page.
    pub objects: Vec<ObjectInfo>,
    /// Continuation token; `None` when this is the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Write options for `Destination.put`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOptions {
    /// MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Known length; omit when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_length: Option<u64>,
    /// Expected SHA-256 of the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<Vec<u8>>,
}

/// Result of a streamed put.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutResult {
    /// Object key written.
    pub key: String,
    /// Bytes accepted.
    pub bytes_written: u64,
    /// Backend etag when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// Digest of the written body when computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<Vec<u8>>,
}

/// Result of a server-side copy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    /// Bytes copied when known.
    pub bytes_copied: u64,
}
