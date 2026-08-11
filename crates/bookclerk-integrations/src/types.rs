//! Integration events and shared types.

use std::path::PathBuf;

use bookclerk_library::BookRecord;
use serde::{Deserialize, Serialize};

/// Events fan-out to registered [`crate::traits::Integration`]s.
#[derive(Debug, Clone)]
pub enum IntegrationEvent {
    /// A title was successfully acquired (or matched existing storage).
    BookAcquired {
        /// Book.
        book: Box<BookRecord>,
        /// Storage key.
        storage_key: String,
        /// Absolute path.
        absolute_path: Option<PathBuf>,
    },
    /// An external identity was observed (e.g. ABS user created).
    ExternalUserObserved {
        /// Provider.
        provider: String,
        /// External user Identifier.
        external_user_id: String,
        /// Display name.
        display_name: Option<String>,
    },
}

/// Health snapshot for one integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationHealth {
    /// Identifier.
    pub id: String,
    /// Enabled.
    pub enabled: bool,
    /// Ok.
    pub ok: bool,
    /// Detail.
    pub detail: Option<String>,
}

/// External user resolved via integration credential login or watcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalUser {
    /// Provider.
    pub provider: String,
    /// External user Identifier.
    pub external_user_id: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Ephemeral token from the remote system (e.g. ABS JWT). Never persisted.
    #[serde(default, skip_serializing)]
    pub access_token: Option<String>,
}

/// One listening / progress row from any integration (ABS, plugins, …).
///
/// Hosts upsert these into the generic `listening_progress` table; ranking never
/// sees the originating adapter—only these fields.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListeningProgressSnapshot {
    /// External user Identifier.
    pub external_user_id: String,
    /// External item Identifier.
    pub external_item_id: String,
    /// Identity Identifier.
    #[serde(default)]
    pub identity_id: Option<i64>,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
    /// Authors.
    #[serde(default)]
    pub authors: Option<String>,
    /// Amazon ASIN identifier.
    #[serde(default)]
    pub asin: Option<String>,
    /// ISBN identifier.
    #[serde(default)]
    pub isbn: Option<String>,
    /// Progress.
    #[serde(default)]
    pub progress: Option<f64>,
    /// Current time seconds.
    #[serde(default)]
    pub current_time_seconds: Option<f64>,
    /// Duration seconds.
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    /// Is finished.
    #[serde(default)]
    pub is_finished: bool,
    /// Last listened at.
    #[serde(default)]
    pub last_listened_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Result of syncing listening progress across one or more integrations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncListeningSummary {
    /// Upserted.
    pub upserted: usize,
    /// By provider.
    pub by_provider: Vec<SyncListeningProviderResult>,
}

/// Per-integration outcome from [`crate::IntegrationRegistry::sync_listening_progress_all`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncListeningProviderResult {
    /// Identifier.
    pub id: String,
    /// Upserted.
    pub upserted: usize,
    /// Error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
