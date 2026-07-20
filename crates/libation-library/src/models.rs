use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Liberate / download state for a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LiberateStatus {
    #[default]
    NotLiberated,
    Queued,
    Downloading,
    Liberated,
    Error,
}

impl LiberateStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotLiberated => "not_liberated",
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Liberated => "liberated",
            Self::Error => "error",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "not_liberated" => Some(Self::NotLiberated),
            "queued" => Some(Self::Queued),
            "downloading" => Some(Self::Downloading),
            "liberated" => Some(Self::Liberated),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Account row stored in the Libation DB (mirrors audible-rs accounts lightly).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRecord {
    pub id: i64,
    pub account_id: String,
    pub marketplace: String,
    pub label: Option<String>,
    pub scan_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Book / library item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookRecord {
    pub id: i64,
    pub asin: String,
    pub account_id: String,
    pub marketplace: String,
    pub title: String,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub liberate_status: LiberateStatus,
    /// Storage key (not necessarily a local path) after liberate.
    pub storage_key: Option<String>,
    pub error_message: Option<String>,
    pub purchased_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
