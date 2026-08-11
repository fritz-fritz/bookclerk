use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Acquire / download state for a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AcquireStatus {
    /// Not acquired variant.
    #[default]
    NotAcquired,
    /// Queued variant.
    Queued,
    /// Downloading variant.
    Downloading,
    /// Acquired variant.
    Acquired,
    /// Error variant.
    Error,
}

impl AcquireStatus {
    /// As str.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotAcquired => "not_acquired",
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Acquired => "acquired",
            Self::Error => "error",
        }
    }

    /// Parse.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "not_acquired" => Some(Self::NotAcquired),
            "queued" => Some(Self::Queued),
            "downloading" => Some(Self::Downloading),
            "acquired" => Some(Self::Acquired),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Classic EF `LiberatedStatus` integer.
    #[must_use]
    pub fn to_classic(self) -> i32 {
        match self {
            Self::Acquired => 1,
            Self::Error => 2,
            Self::Downloading | Self::Queued => 0x1000,
            Self::NotAcquired => 0,
        }
    }

    /// Parse classic EF `LiberatedStatus` integer.
    #[must_use]
    pub fn from_classic(status: i64) -> Self {
        match status {
            1 => Self::Acquired,
            2 => Self::Error,
            0x1000 => Self::Downloading,
            _ => Self::NotAcquired,
        }
    }
}

/// True when `content_kind` is a podcast episode.
#[must_use]
pub fn is_episode(content_kind: &str) -> bool {
    content_kind.eq_ignore_ascii_case("episode")
}

/// True when `content_kind` is a podcast parent / show (no audio to acquire).
#[must_use]
pub fn is_podcast_parent(content_kind: &str) -> bool {
    matches!(
        content_kind.to_ascii_lowercase().as_str(),
        "podcast" | "parent" | "podcastparent" | "podcast_parent" | "season"
    )
}

/// True when the title can be downloaded (classic `WithoutParents`).
#[must_use]
pub fn is_downloadable(content_kind: &str) -> bool {
    !is_podcast_parent(content_kind)
}

/// Classic EF `ContentType` enum value.
#[must_use]
pub fn content_kind_to_classic(content_kind: &str) -> i32 {
    if is_episode(content_kind) {
        2 // Episode
    } else if is_podcast_parent(content_kind) {
        4 // Parent
    } else {
        1 // Product
    }
}

/// Map classic EF `ContentType` to bookclerk `content_kind`.
#[must_use]
pub fn content_kind_from_classic(content_type: i64) -> String {
    match content_type {
        2 => String::from("episode"),
        4 => String::from("podcast"),
        _ => String::from("book"),
    }
}

/// Account row stored in the Bookclerk DB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRecord {
    /// Identifier.
    pub id: i64,
    /// Account Identifier.
    pub account_id: String,
    /// `audible` or `libro`.
    pub source: String,
    /// Marketplace.
    pub marketplace: String,
    /// Label.
    pub label: Option<String>,
    /// Scan enabled.
    pub scan_enabled: bool,
    /// `active` or `revoked` (credentials removed; books retained).
    #[serde(default = "default_connection_status")]
    pub connection_status: String,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

fn default_connection_status() -> String {
    String::from("active")
}

/// Portal identity bound to an external provider user (e.g. ABS).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalIdentity {
    /// Identifier.
    pub id: i64,
    /// Provider.
    pub provider: String,
    /// External user Identifier.
    pub external_user_id: String,
    /// Label.
    pub label: Option<String>,
    /// First-party user this external identity is linked to (Phase 1).
    pub user_id: Option<i64>,
    /// Created at.
    pub created_at: DateTime<Utc>,
}

/// First-party user role (`administrator` | `member`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    /// Administrator variant.
    Administrator,
    /// Member variant.
    Member,
}

impl UserRole {
    /// As str.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Administrator => "administrator",
            Self::Member => "member",
        }
    }

    /// Parse.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "administrator" => Some(Self::Administrator),
            "member" => Some(Self::Member),
            _ => None,
        }
    }
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// First-party user status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    /// Active variant.
    Active,
    /// Disabled variant.
    Disabled,
}

impl UserStatus {
    /// As str.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    /// Parse.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// First-party Bookclerk user (security principal for portal paths).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecord {
    /// Identifier.
    pub id: i64,
    /// Role.
    pub role: UserRole,
    /// Status.
    pub status: UserStatus,
    /// Display name.
    pub display_name: Option<String>,
    /// Login name.
    pub login_name: Option<String>,
    /// Has password.
    pub has_password: bool,
    /// Security version.
    pub security_version: i64,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

/// Invite ticket for provisioning a User (token plaintext never stored).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInviteRecord {
    /// Identifier.
    pub id: i64,
    /// Token hash.
    pub token_hash: String,
    /// Role.
    pub role: UserRole,
    /// Login name.
    pub login_name: Option<String>,
    /// Display name.
    pub display_name: Option<String>,
    /// Expires at.
    pub expires_at: DateTime<Utc>,
    /// Redeemed at.
    pub redeemed_at: Option<DateTime<Utc>>,
    /// Created by.
    pub created_by: String,
    /// Created at.
    pub created_at: DateTime<Utc>,
}

/// Durable operator session metadata (hashed token is never exposed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorSessionRecord {
    /// Identifier.
    pub id: i64,
    /// Expires at.
    pub expires_at: DateTime<Utc>,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Last used at.
    pub last_used_at: Option<DateTime<Utc>>,
    /// Elevated from user Identifier.
    pub elevated_from_user_id: Option<i64>,
    /// Impersonating user Identifier.
    pub impersonating_user_id: Option<i64>,
}

/// Security audit event (elevate / impersonate / login / provision).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAuditEvent {
    /// Identifier.
    pub id: i64,
    /// At.
    pub at: DateTime<Utc>,
    /// Actor.
    pub actor: String,
    /// Action.
    pub action: String,
    /// Detail JSON.
    pub detail_json: Option<String>,
}

/// Claim ticket metadata (token plaintext is never stored).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimTicketRecord {
    /// Identifier.
    pub id: i64,
    /// Token hash.
    pub token_hash: String,
    /// Identity Identifier.
    pub identity_id: Option<i64>,
    /// Expires at.
    pub expires_at: DateTime<Utc>,
    /// Redeemed at.
    pub redeemed_at: Option<DateTime<Utc>>,
    /// Created by.
    pub created_by: String,
    /// Created at.
    pub created_at: DateTime<Utc>,
}

/// Link between a portal identity and a bookstore account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountLinkRecord {
    /// Identifier.
    pub id: i64,
    /// Identity Identifier.
    pub identity_id: i64,
    /// Account Identifier.
    pub account_id: String,
    /// Source.
    pub source: String,
    /// Created at.
    pub created_at: DateTime<Utc>,
}

/// Book / library item (one ownership row per store product per account).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookRecord {
    /// Identifier.
    pub id: i64,
    /// Public stable id (CLI / API / acquire target).
    pub uuid: String,
    /// `audible` or `libro`.
    pub source: String,
    /// Account Identifier.
    pub account_id: String,
    /// Source-native product key (Audible ASIN or Libro ISBN).
    pub product_id: String,
    /// Audible ASIN when known (`None` for Libro-only rows without enrichment).
    pub asin: Option<String>,
    /// ISBN-13 when known (Libro always; Audible when the API provides it).
    pub isbn: Option<String>,
    /// Marketplace.
    pub marketplace: String,
    /// Title.
    pub title: String,
    /// Authors.
    pub authors: Option<String>,
    /// Narrators.
    pub narrators: Option<String>,
    /// Series.
    pub series: Option<String>,
    /// Series index.
    pub series_index: Option<String>,
    /// Audible series / podcast-parent ASIN (`Series.AudibleSeriesId`).
    pub series_asin: Option<String>,
    /// Acquire status.
    pub acquire_status: AcquireStatus,
    /// Storage key (not necessarily a local path) after acquire.
    pub storage_key: Option<String>,
    /// Error message.
    pub error_message: Option<String>,
    /// Purchased at.
    pub purchased_at: Option<DateTime<Utc>>,
    /// Space-separated user tags (classic `UserDefinedItem.Tags`).
    pub tags: Option<String>,
    /// Rating overall.
    pub rating_overall: Option<f32>,
    /// Rating performance.
    pub rating_performance: Option<f32>,
    /// Rating story.
    pub rating_story: Option<f32>,
    /// Is finished.
    pub is_finished: bool,
    /// Pdf status.
    pub pdf_status: AcquireStatus,
    /// Pdf storage key.
    pub pdf_storage_key: Option<String>,
    /// Publisher.
    pub publisher: Option<String>,
    /// Length minutes.
    pub length_minutes: Option<i64>,
    /// Is abridged.
    pub is_abridged: bool,
    /// `book`, `episode`, `podcast`, etc. (classic scan metadata).
    pub content_kind: String,
    /// Categories.
    pub categories: Option<String>,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Published at.
    pub published_at: Option<DateTime<Utc>>,
    /// Blurb / description from enrichment or store APIs.
    pub description: Option<String>,
    /// Language.
    pub language: Option<String>,
    /// Cover URL.
    pub cover_url: Option<String>,
    /// Subject / topic tags (often from Open Library; `;`- or `,`-separated).
    pub subjects: Option<String>,
    /// Last enrichment provider (`audible`, `openlibrary`, …).
    pub enrich_source: Option<String>,
    /// Enrich confidence.
    pub enrich_confidence: Option<f64>,
    /// Enrich updated at.
    pub enrich_updated_at: Option<DateTime<Utc>>,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

/// Canonical work spanning one or more ownership rows / editions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkRecord {
    /// Identifier.
    pub id: String,
    /// Canonical Amazon ASIN identifier.
    pub canonical_asin: Option<String>,
    /// Canonical ISBN identifier.
    pub canonical_isbn: Option<String>,
    /// Title.
    pub title: String,
    /// Authors.
    pub authors: Option<String>,
    /// Narrators.
    pub narrators: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Subjects.
    pub subjects: Option<String>,
    /// Categories.
    pub categories: Option<String>,
    /// Language.
    pub language: Option<String>,
    /// Series.
    pub series: Option<String>,
    /// Series index.
    pub series_index: Option<String>,
    /// Cover URL.
    pub cover_url: Option<String>,
    /// Openlibrary Identifier.
    pub openlibrary_id: Option<String>,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

/// Listening progress snapshot from an external player (e.g. AudioBookshelf).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListeningProgressRecord {
    /// Identifier.
    pub id: i64,
    /// Identity Identifier.
    pub identity_id: Option<i64>,
    /// Provider.
    pub provider: String,
    /// External user Identifier.
    pub external_user_id: String,
    /// Book UUID.
    pub book_uuid: Option<String>,
    /// Work Identifier.
    pub work_id: Option<String>,
    /// External item Identifier.
    pub external_item_id: String,
    /// Title.
    pub title: Option<String>,
    /// Authors.
    pub authors: Option<String>,
    /// Amazon ASIN identifier.
    pub asin: Option<String>,
    /// ISBN identifier.
    pub isbn: Option<String>,
    /// Progress.
    pub progress: Option<f64>,
    /// Current time seconds.
    pub current_time_seconds: Option<f64>,
    /// Duration seconds.
    pub duration_seconds: Option<f64>,
    /// Is finished.
    pub is_finished: bool,
    /// Last listened at.
    pub last_listened_at: Option<DateTime<Utc>>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

/// Wishlist row status (`open` while wishlisted; `cancelled` after un-wishlist).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    /// Open variant.
    #[default]
    Open,
    /// Cancelled variant.
    Cancelled,
}

impl RequestStatus {
    /// As str.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            // Legacy triage statuses collapse to cancelled (no approval flow).
            "cancelled" | "approved" | "acquired" | "rejected" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Per-storefront catalog/pricing snapshot attached to a wishlist row.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TitleRequestSourceRecord {
    /// Identifier.
    pub id: i64,
    /// Title request Identifier.
    pub title_request_id: i64,
    /// Source.
    pub source: String,
    /// Product Identifier.
    pub product_id: String,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Subtitle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Authors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<String>,
    /// Narrators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrators: Option<String>,
    /// Series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    /// Series index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_index: Option<String>,
    /// Amazon ASIN identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asin: Option<String>,
    /// ISBN identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isbn: Option<String>,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Publisher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Length minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_minutes: Option<i64>,
    /// Published at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// Categories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<String>,
    /// Language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Cover URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    /// URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Price cents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_cents: Option<i64>,
    /// Currency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Price label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_label: Option<String>,
    /// List price cents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_price_cents: Option<i64>,
    /// List price label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_price_label: Option<String>,
    /// Member price cents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_price_cents: Option<i64>,
    /// Member price label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_price_label: Option<String>,
    /// Observed at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

/// Storefront edition key for wishlist / queue payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WishlistStoreEdition {
    /// Source.
    pub source: String,
    /// Product Identifier.
    pub product_id: String,
}

/// Snapshotted purchase link/price for a wishlist title.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WishlistPurchaseHint {
    /// Source.
    pub source: String,
    /// Product Identifier.
    pub product_id: String,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Price cents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_cents: Option<i64>,
    /// Currency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Price label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_label: Option<String>,
    /// List price cents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_price_cents: Option<i64>,
    /// List price label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_price_label: Option<String>,
    /// Member price cents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_price_cents: Option<i64>,
    /// Member price label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_price_label: Option<String>,
}

/// Personal wishlist row (also contributes to the shared global queue while open).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleRequestRecord {
    /// Identifier.
    pub id: i64,
    /// UUID.
    pub uuid: String,
    /// `None` = operator-submitted.
    pub identity_id: Option<i64>,
    /// Title.
    pub title: String,
    /// Authors.
    pub authors: Option<String>,
    /// Amazon ASIN identifier.
    pub asin: Option<String>,
    /// ISBN identifier.
    pub isbn: Option<String>,
    /// Notes.
    pub notes: Option<String>,
    /// Status.
    pub status: RequestStatus,
    /// Stable bibliographic key (`isbn:…` / `asin:…` / `soft:…`) for aggregation.
    #[serde(default)]
    pub work_key: String,
    /// Work Identifier.
    pub work_id: Option<String>,
    /// Resolved book UUID.
    pub resolved_book_uuid: Option<String>,
    /// Cover URL.
    pub cover_url: Option<String>,
    /// Per-storefront snapshots (empty for legacy rows).
    #[serde(default)]
    pub sources: Vec<TitleRequestSourceRecord>,
    /// Merged from [`Self::sources`] (HTML preferred).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Subtitle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Narrators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrators: Option<String>,
    /// Series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    /// Series index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_index: Option<String>,
    /// Publisher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Length minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_minutes: Option<i64>,
    /// Published at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// Genres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genres: Option<String>,
    /// Language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Store editions.
    #[serde(default)]
    pub store_editions: Vec<WishlistStoreEdition>,
    /// Purchase hints.
    #[serde(default)]
    pub purchase_hints: Vec<WishlistPurchaseHint>,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

/// Aggregated global request-queue entry (one work, many wishers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalQueueEntry {
    /// Work key.
    pub work_key: String,
    /// Title.
    pub title: String,
    /// Authors.
    pub authors: Option<String>,
    /// Amazon ASIN identifier.
    pub asin: Option<String>,
    /// ISBN identifier.
    pub isbn: Option<String>,
    /// Cover URL.
    pub cover_url: Option<String>,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Subtitle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Narrators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrators: Option<String>,
    /// Series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    /// Series index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_index: Option<String>,
    /// Publisher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Length minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_minutes: Option<i64>,
    /// Published at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// Genres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genres: Option<String>,
    /// Language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Store editions.
    #[serde(default)]
    pub store_editions: Vec<WishlistStoreEdition>,
    /// Purchase hints.
    #[serde(default)]
    pub purchase_hints: Vec<WishlistPurchaseHint>,
    /// Wish count.
    pub wish_count: i64,
    /// Sample uuids.
    pub sample_uuids: Vec<String>,
    /// First requested at.
    pub first_requested_at: DateTime<Utc>,
    /// Last requested at.
    pub last_requested_at: DateTime<Utc>,
}

/// Stored embedding vector metadata (blob fetched separately when needed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRecord {
    /// Identifier.
    pub id: i64,
    /// Target kind.
    pub target_kind: String,
    /// Target Identifier.
    pub target_id: String,
    /// Model.
    pub model: String,
    /// Dims.
    pub dims: i64,
    /// Text hash.
    pub text_hash: String,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

/// Per-user GUI / Discover preferences (operator or portal identity).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    /// Identifier.
    pub id: i64,
    /// `operator` or `portal:{identity_id}`.
    pub subject_key: String,
    /// Identity Identifier.
    pub identity_id: Option<i64>,
    /// `discover` | `library` | `accounts`.
    pub default_view: String,
    /// Shelf kind ids to hide (`author`, `chirp_deals`, …). Empty = all on.
    pub disabled_shelves: Vec<String>,
    /// Catalog search sort key (`relevance`, `popularity`, …).
    pub discover_sort: String,
    /// `asc` | `desc`.
    pub discover_sort_dir: String,
    /// Preferred content language (`en`, `__all__`, …). `None` = browser default.
    pub discover_language: Option<String>,
    /// Store ids to hide in Discover. Empty = all sources (including future).
    pub discover_excluded_sources: Vec<String>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

impl UserPreferences {
    /// Defaults when no row exists yet.
    #[must_use]
    pub fn defaults_for(subject_key: &str, identity_id: Option<i64>) -> Self {
        Self {
            id: 0,
            subject_key: subject_key.to_string(),
            identity_id,
            default_view: String::from("discover"),
            disabled_shelves: Vec::new(),
            discover_sort: String::from("relevance"),
            discover_sort_dir: String::from("desc"),
            discover_language: None,
            discover_excluded_sources: Vec::new(),
            updated_at: Utc::now(),
        }
    }
}

/// Subject key for the shared operator account.
pub const OPERATOR_PREFS_KEY: &str = "operator";

/// Subject key for a portal identity (legacy; prefer [`user_prefs_key`]).
#[must_use]
pub fn portal_prefs_key(identity_id: i64) -> String {
    format!("portal:{identity_id}")
}

/// Subject key for a first-party user.
#[must_use]
pub fn user_prefs_key(user_id: i64) -> String {
    format!("user:{user_id}")
}

impl BookRecord {
    /// Public stable id used for CLI / API / acquire lookups.
    #[must_use]
    pub fn title_id(&self) -> &str {
        &self.uuid
    }

    /// Naming / display fallback: ASIN, else ISBN, else source product id.
    #[must_use]
    pub fn asin_or_isbn(&self) -> &str {
        self.asin
            .as_deref()
            .or(self.isbn.as_deref())
            .unwrap_or(self.product_id.as_str())
    }

    /// Source-native id for download / fetch APIs (`product_id`).
    ///
    /// Always the owning store's id (Audible ASIN or Libro ISBN), never an
    /// enrichment ASIN copied onto a non-Audible row.
    #[must_use]
    pub fn download_product_id(&self) -> &str {
        self.product_id.as_str()
    }

    /// Audible ASIN for Audible license / catalog APIs when one is known.
    #[must_use]
    pub fn audible_asin(&self) -> Option<&str> {
        self.asin.as_deref()
    }
}
