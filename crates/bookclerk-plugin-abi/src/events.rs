//! Typed host↔plugin event envelopes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Host → plugin (`onEvent`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum HostToPluginEvent {
    /// Book acquired variant.
    #[serde(rename = "book_acquired")]
    BookAcquired(BookAcquiredPayload),
    /// Library scan completed variant.
    #[serde(rename = "library_scan_completed")]
    LibraryScanCompleted(LibraryScanCompletedPayload),
    /// Config changed variant.
    #[serde(rename = "config_changed")]
    ConfigChanged(ConfigChangedPayload),
}

/// Book acquired payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BookAcquiredPayload {
    /// Title Identifier.
    pub title_id: String,
    /// Source.
    pub source: String,
    /// Amazon ASIN identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asin: Option<String>,
    /// ISBN identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isbn: Option<String>,
    /// Path keys.
    pub path_keys: Vec<String>,
}

/// Library scan completed payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryScanCompletedPayload {
    /// Source.
    pub source: String,
    /// Upserted.
    pub upserted: u64,
}

/// Config changed payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChangedPayload {
    /// Config.
    pub config: Value,
}

/// Plugin → host (`env.HOST.notify`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum PluginToHostEvent {
    /// External users variant.
    #[serde(rename = "external_users")]
    ExternalUsers(ExternalUsersPayload),
    /// Listening progress variant.
    #[serde(rename = "listening_progress")]
    ListeningProgress(ListeningProgressPayload),
    /// Plugin log variant.
    #[serde(rename = "plugin_log")]
    PluginLog(PluginLogPayload),
}

/// External users payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalUsersPayload {
    /// Users.
    pub users: Vec<Value>,
}

/// Listening progress payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListeningProgressPayload {
    /// Items.
    pub items: Vec<Value>,
}

/// Plugin log payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginLogPayload {
    /// Level.
    pub level: PluginLogLevel,
    /// Message.
    pub message: String,
}

/// Plugin log level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PluginLogLevel {
    /// Debug variant.
    Debug,
    /// Info variant.
    Info,
    /// Warn variant.
    Warn,
    /// Error variant.
    Error,
}
