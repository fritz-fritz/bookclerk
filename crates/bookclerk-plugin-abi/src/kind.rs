//! Kind-specific Workers RPC DTOs (source / integration / output).
//!
//! Field names serialize as camelCase on the wire. Tagged enums keep their
//! discriminant rename policy (`SourceFetchDto` variant tags stay
//! `snake_case`; `DbConnectParams` backend tags stay `lowercase`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_true() -> bool {
    true
}

fn default_page() -> u32 {
    50
}

fn default_catalog_limit() -> usize {
    20
}

fn default_catalog_page() -> u32 {
    1
}

/// Book-acquired event payload (host → plugin).
///
/// `book` is opaque JSON shaped like the host library row; guests may deserialize
/// a subset without depending on `bookclerk-library`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookAcquiredDto {
    /// Book.
    pub book: Value,
    /// Storage key.
    pub storage_key: String,
    /// Absolute path.
    #[serde(default)]
    pub absolute_path: Option<String>,
}

/// Source account DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAccountDto {
    /// Account Identifier.
    pub account_id: String,
    /// Source.
    pub source: String,
    /// Marketplace.
    pub marketplace: String,
    /// Label.
    #[serde(default)]
    pub label: Option<String>,
    /// Scan enabled.
    #[serde(default = "default_true")]
    pub scan_enabled: bool,
}

/// Login params for source plugins (no files-dir root / DB path).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginParams {
    /// Scoped writable directory for this plugin only (`…/plugins/<id>/data`).
    pub plugin_data_dir: String,
    /// Marketplace.
    #[serde(default)]
    pub marketplace: String,
    /// Label.
    #[serde(default)]
    pub label: Option<String>,
    /// Email.
    #[serde(default)]
    pub email: Option<String>,
    /// Password.
    #[serde(default)]
    pub password: Option<String>,
    /// Force.
    #[serde(default)]
    pub force: bool,
    /// Optional bind address for OAuth callback servers (`host:port`).
    /// Ignored when [`Self::callback_ipc`] is set (host owns the TCP listener).
    #[serde(default)]
    pub callback_bind: Option<String>,
    /// Host-owned callback IPC endpoint the guest must connect to.
    ///
    /// When set (with [`Self::callback_public_base`]), the guest must **not**
    /// bind a TCP listener. The host accepts browser connections and forwards
    /// raw bytes over this duplex IPC (Unix socket path or Windows pipe name).
    #[serde(default)]
    pub callback_ipc: Option<String>,
    /// Public base URL for the host TCP listener, e.g. `http://127.0.0.1:12345`.
    /// Combined with the guest's landing path to form the browser URL.
    #[serde(default)]
    pub callback_public_base: Option<String>,
    /// External / paste-redirect OAuth instead of a local callback server.
    #[serde(default)]
    pub external: bool,
    /// Pre-supplied OAuth redirect URL.
    #[serde(default)]
    pub response_url: Option<String>,
    /// Prefer QR output when the guest supports it.
    #[serde(default)]
    pub show_qr: bool,
    /// Seconds to wait for OAuth callback capture.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Store-specific knobs; guests may ignore unknowns.
    #[serde(default)]
    pub extra: Value,
}

/// Login result — account metadata plus opaque credentials for the host to seal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResultDto {
    /// Account.
    pub account: SourceAccountDto,
    /// Opaque JSON credential blob. Host seals into `encrypted_secrets`
    /// (`provider = plugin id`). Never written by the plugin into the library DB.
    #[serde(default)]
    pub credentials: Option<Value>,
}

/// Result of [`crate::methods::login_start`] (interactive OAuth).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStartResultDto {
    /// Opaque session id for [`crate::methods::login_complete`].
    pub session_id: String,
    /// Browser URL the operator should open.
    pub url: String,
}

/// Params for [`crate::methods::login_complete`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCompleteParams {
    /// Session Identifier.
    pub session_id: String,
}

/// Params for [`crate::methods::credentials_update`] — guest-requested credential write-back.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsUpdateParams {
    /// Account Identifier.
    pub account_id: String,
    /// Replacement opaque credential JSON for the host to re-seal.
    pub credentials: Value,
}

/// One external user observed by an integration (ABS-only concerns; host workflows).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalUserDto {
    /// Provider.
    pub provider: String,
    /// External user Identifier.
    pub external_user_id: String,
    /// Display name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Ephemeral remote token (e.g. ABS JWT). Guest→host only; never persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
}

/// Result of [`crate::methods::poll_events`] — signals for the host to kick off workflows.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EventPollResultDto {
    /// Users.
    #[serde(default)]
    pub users: Vec<ExternalUserDto>,
}

/// Scan params — no library DB path.
///
/// Host injects sealed credentials (same mediation as `fetch_title`) so the
/// plugin does not need a private credential store under `plugin_data_dir`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanParams {
    /// Plugin data dir.
    pub plugin_data_dir: String,
    /// Accounts.
    #[serde(default)]
    pub accounts: Vec<String>,
    /// Page size.
    #[serde(default = "default_page")]
    pub page_size: u32,
    /// Import episodes.
    #[serde(default = "default_true")]
    pub import_episodes: bool,
    /// Import plus titles.
    #[serde(default = "default_true")]
    pub import_plus_titles: bool,
    /// Host-loaded credential blobs keyed by `account_id`.
    ///
    /// Values are the same opaque JSON sealed at `login`. Empty when the host
    /// has no credentials for the requested accounts.
    #[serde(default)]
    pub credentials: std::collections::BTreeMap<String, Value>,
}

/// One library title returned by an external source `scan` (host upserts).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanBookDto {
    /// Account Identifier.
    pub account_id: String,
    /// Product Identifier.
    pub product_id: String,
    /// Title.
    pub title: String,
    /// Marketplace.
    #[serde(default)]
    pub marketplace: Option<String>,
    /// Amazon ASIN identifier.
    #[serde(default)]
    pub asin: Option<String>,
    /// ISBN identifier.
    #[serde(default)]
    pub isbn: Option<String>,
    /// Authors.
    #[serde(default)]
    pub authors: Option<String>,
    /// Narrators.
    #[serde(default)]
    pub narrators: Option<String>,
    /// Series.
    #[serde(default)]
    pub series: Option<String>,
    /// Series index.
    #[serde(default)]
    pub series_index: Option<String>,
    /// Content kind.
    #[serde(default)]
    pub content_kind: Option<String>,
    /// Publisher.
    #[serde(default)]
    pub publisher: Option<String>,
    /// Length minutes.
    #[serde(default)]
    pub length_minutes: Option<i64>,
    /// Subtitle.
    #[serde(default)]
    pub subtitle: Option<String>,
}

/// Scan summary Wire DTO.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummaryDto {
    /// Accounts.
    #[serde(default)]
    pub accounts: usize,
    /// Books upserted.
    #[serde(default)]
    pub books_upserted: usize,
    /// Pages.
    #[serde(default)]
    pub pages: u32,
    /// Skipped disabled.
    #[serde(default)]
    pub skipped_disabled: usize,
    /// Titles for the host to upsert. Prefer this over plugin-side DB writes.
    #[serde(default)]
    pub books: Vec<ScanBookDto>,
}

/// Fetch-title params. Plugin writes media under `cache_dir` and returns paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchTitleParams {
    /// Plugin data dir.
    pub plugin_data_dir: String,
    /// Account Identifier.
    pub account_id: String,
    /// Title Identifier.
    pub title_id: String,
    /// Cache dir.
    pub cache_dir: String,
    /// Host-loaded credential blob for this account (sealed in DB; plugin never opens DB).
    #[serde(default)]
    pub credentials: Option<Value>,
    /// Opaque plugin table from `[sources.<id>]`.
    #[serde(default)]
    pub source_config: Value,
    /// Host acquire/download options (JSON object matching host `DownloadOptions`).
    ///
    /// Guests should honor fetch-relevant knobs (`widevine`, `strip_audible_brand_audio`,
    /// `download_cover`, `chapter_layout`, speed limits, …) so external load matches
    /// in-process. Plugin-specific overlays (e.g. `[sources.audible].bitrate`) still
    /// come from [`Self::source_config`].
    #[serde(default)]
    pub download: Value,
}

/// Plain (DRM-free) fetch result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceFetchDto {
    /// Plain variant.
    #[serde(rename_all = "camelCase")]
    Plain {
        /// Parts.
        parts: Vec<PlainPartDto>,
        /// M4b path.
        #[serde(default)]
        m4b_path: Option<String>,
        /// Cover path.
        #[serde(default)]
        cover_path: Option<String>,
        /// Chapters.
        #[serde(default)]
        chapters: Vec<(String, u64)>,
        /// Companion PDF download URL when the store exposes one.
        #[serde(default)]
        pdf_url: Option<String>,
    },
}

/// Params for [`crate::methods::catalog_detail`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDetailParams {
    /// Store product id (Libro ISBN or ISBN-slug).
    pub product_id: String,
    /// Optional ISBN when it differs from [`Self::product_id`].
    #[serde(default)]
    pub isbn: Option<String>,
}

/// Params for [`crate::methods::search_catalog`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchCatalogParams {
    /// Query.
    pub query: String,
    /// Region.
    #[serde(default)]
    pub region: String,
    /// Limit.
    #[serde(default = "default_catalog_limit")]
    pub limit: usize,
    /// 1-based page for storefronts that page (default 1).
    #[serde(default = "default_catalog_page")]
    pub page: u32,
    /// `relevance` / `popularity` / `rating` / `title` / `author`.
    #[serde(default)]
    pub sort: Option<String>,
    /// Optional facet (`author` / `narrator` / `series` / `genre`).
    #[serde(default)]
    pub field: Option<String>,
    /// Preferred content language (soft-prioritize; e.g. `en`).
    #[serde(default)]
    pub language: Option<String>,
}

/// Params for [`crate::methods::expand_candidates`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExpandCandidatesParams {
    /// Source.
    #[serde(default)]
    pub source: String,
    /// Product Identifier.
    #[serde(default)]
    pub product_id: String,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Authors.
    #[serde(default)]
    pub authors: Option<String>,
    /// Narrators.
    #[serde(default)]
    pub narrators: Option<String>,
    /// Series.
    #[serde(default)]
    pub series: Option<String>,
    /// Series Amazon ASIN identifier.
    #[serde(default)]
    pub series_asin: Option<String>,
    /// Amazon ASIN identifier.
    #[serde(default)]
    pub asin: Option<String>,
    /// ISBN identifier.
    #[serde(default)]
    pub isbn: Option<String>,
    /// Region.
    #[serde(default)]
    pub region: String,
    /// Limit.
    #[serde(default = "default_catalog_limit")]
    pub limit: usize,
}

/// Params for [`crate::methods::purchase_hint`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseHintParams {
    /// Product Identifier.
    #[serde(default)]
    pub product_id: Option<String>,
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
    /// Region.
    #[serde(default)]
    pub region: String,
    /// With price.
    #[serde(default)]
    pub with_price: bool,
}

/// Wire form of a catalog / candidate hit.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogHitDto {
    /// Product Identifier.
    pub product_id: String,
    /// Title.
    pub title: String,
    /// Authors.
    #[serde(default)]
    pub authors: Option<String>,
    /// Narrators.
    #[serde(default)]
    pub narrators: Option<String>,
    /// Series.
    #[serde(default)]
    pub series: Option<String>,
    /// Series index.
    #[serde(default)]
    pub series_index: Option<String>,
    /// Amazon ASIN identifier.
    #[serde(default)]
    pub asin: Option<String>,
    /// ISBN identifier.
    #[serde(default)]
    pub isbn: Option<String>,
    /// URL.
    #[serde(default)]
    pub url: Option<String>,
    /// Cover URL.
    #[serde(default)]
    pub cover_url: Option<String>,
    /// Origin.
    #[serde(default)]
    pub origin: String,
    /// Subtitle.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// Publisher.
    #[serde(default)]
    pub publisher: Option<String>,
    /// Length minutes.
    #[serde(default)]
    pub length_minutes: Option<i64>,
    /// Published at.
    #[serde(default)]
    pub published_at: Option<String>,
    /// Categories.
    #[serde(default)]
    pub categories: Option<String>,
    /// Language.
    #[serde(default)]
    pub language: Option<String>,
    /// Price cents.
    #[serde(default)]
    pub price_cents: Option<i64>,
    /// Currency.
    #[serde(default)]
    pub currency: Option<String>,
    /// Price label.
    #[serde(default)]
    pub price_label: Option<String>,
    /// Rating overall.
    #[serde(default)]
    pub rating_overall: Option<f64>,
    /// Rating count.
    #[serde(default)]
    pub rating_count: Option<i64>,
    /// Is abridged.
    #[serde(default)]
    pub is_abridged: Option<bool>,
}

/// Wire form of `SourcePurchaseHint`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseHintDto {
    /// Product Identifier.
    pub product_id: String,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
    /// URL.
    #[serde(default)]
    pub url: Option<String>,
    /// Price cents.
    #[serde(default)]
    pub price_cents: Option<i64>,
    /// Currency.
    #[serde(default)]
    pub currency: Option<String>,
    /// Price label.
    #[serde(default)]
    pub price_label: Option<String>,
    /// List price cents.
    #[serde(default)]
    pub list_price_cents: Option<i64>,
    /// List price label.
    #[serde(default)]
    pub list_price_label: Option<String>,
    /// Member price cents.
    #[serde(default)]
    pub member_price_cents: Option<i64>,
    /// Member price label.
    #[serde(default)]
    pub member_price_label: Option<String>,
}

/// Plain part Wire DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlainPartDto {
    /// Path.
    pub path: String,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
    /// Duration ms.
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

/// Listening / progress rows returned by integration capability `sync_listening`.
///
/// Host upserts into the generic `listening_progress` table tagged with the
/// plugin id; plugins must not open the library DB themselves.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncListeningResultDto {
    /// Items.
    #[serde(default)]
    pub items: Vec<ListeningProgressDto>,
}

/// Wire DTO for one listening-progress row (serde-compatible with the host
/// `ListeningProgressSnapshot` shape).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListeningProgressDto {
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

/// Object metadata for output plugins (mirrors host `bookclerk_storage::ObjectMeta`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ObjectMetaDto {
    /// Content type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Content length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_length: Option<u64>,
    /// Amazon ASIN identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asin: Option<String>,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_time: Option<String>,
    /// Last write time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_write_time: Option<String>,
}

/// Listing entry for output plugins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfoDto {
    /// Key.
    pub key: String,
    /// Size.
    pub size: u64,
}

/// Probe result for output plugins.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ObjectProbeDto {
    /// Key.
    pub key: String,
    /// Size.
    pub size: u64,
    /// Content type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Meta.
    #[serde(default)]
    pub meta: ObjectMetaDto,
}

/// Static AWS-style credentials injected by the host (never read from guest env).
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct S3CredentialsDto {
    /// Access key Identifier.
    pub access_key_id: String,
    /// Secret access key.
    pub secret_access_key: String,
    /// Session token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

impl std::fmt::Debug for S3CredentialsDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3CredentialsDto")
            .field("access_key_id", &"***")
            .field("secret_access_key", &"***")
            .field("session_token", &self.session_token.as_ref().map(|_| "***"))
            .finish()
    }
}

/// S3 destination knobs the host injects on every output RPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutputS3ContextDto {
    /// Scoped writable directory for this plugin only (`…/plugins/<id>/data`).
    pub plugin_data_dir: String,
    /// Bucket.
    pub bucket: String,
    /// Prefix.
    pub prefix: String,
    /// Region.
    pub region: String,
    /// Endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Force path style.
    #[serde(default)]
    pub force_path_style: bool,
    /// When absent the guest may use the AWS SDK default provider chain (unconfined dev only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<S3CredentialsDto>,
}

/// Local filesystem destination knobs the host injects on every output RPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutputLocalContextDto {
    /// Scoped writable directory for this plugin only (`…/plugins/<id>/data`).
    pub plugin_data_dir: String,
    /// Library output root (`[output.local].root`).
    pub root: String,
    /// Key prefix under `root` (`[output.local].prefix`).
    pub prefix: String,
}

/// Params for [`crate::methods::put`] (local filesystem output).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPutParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputLocalContextDto,
    /// Key.
    pub key: String,
    /// Data base64.
    pub data_base64: String,
    /// Meta.
    #[serde(default)]
    pub meta: ObjectMetaDto,
}

/// Params for [`crate::methods::put_file`] (local filesystem output).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPutFileParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputLocalContextDto,
    /// Key.
    pub key: String,
    /// Meta.
    #[serde(default)]
    pub meta: ObjectMetaDto,
    /// Local path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
}

/// Params for [`crate::methods::get`] (local filesystem output).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalGetParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputLocalContextDto,
    /// Key.
    pub key: String,
}

/// Params for key-scoped local output methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalKeyParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputLocalContextDto,
    /// Key.
    pub key: String,
}

/// Params for [`crate::methods::list`] (local filesystem output).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalListParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputLocalContextDto,
    /// Prefix.
    pub prefix: String,
}

/// Params for [`crate::methods::copy`] (local filesystem output).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCopyParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputLocalContextDto,
    /// From.
    pub from: String,
    /// To.
    pub to: String,
}

/// Params for [`crate::methods::touch_file`] (local filesystem output).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTouchFileParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputLocalContextDto,
    /// Key.
    pub key: String,
    /// Created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

/// Params for [`crate::methods::put`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputS3ContextDto,
    /// Key.
    pub key: String,
    /// Base64-encoded object body (sidecars and small objects).
    pub data_base64: String,
    /// Meta.
    #[serde(default)]
    pub meta: ObjectMetaDto,
}

/// Params for [`crate::methods::put_file`].
///
/// Jailed guests receive the local file over the side channel (`SCM_RIGHTS` on
/// the socket at fd 3) immediately before this RPC. When no side channel is
/// wired (e.g. unconfined / best-effort), the host sets [`Self::local_path`]
/// instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutFileParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputS3ContextDto,
    /// Key.
    pub key: String,
    /// Meta.
    #[serde(default)]
    pub meta: ObjectMetaDto,
    /// Local path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
}

/// Params for [`crate::methods::get`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputS3ContextDto,
    /// Key.
    pub key: String,
}

/// Result of [`crate::methods::get`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResultDto {
    /// Data base64.
    pub data_base64: String,
}

/// Params for key-scoped output methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputS3ContextDto,
    /// Key.
    pub key: String,
}

/// Params for [`crate::methods::list`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputS3ContextDto,
    /// Prefix.
    pub prefix: String,
}

/// Params for [`crate::methods::copy`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputS3ContextDto,
    /// From.
    pub from: String,
    /// To.
    pub to: String,
}

/// Params for [`crate::methods::touch_file`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TouchFileParams {
    /// Ctx.
    #[serde(flatten)]
    pub ctx: OutputS3ContextDto,
    /// Key.
    pub key: String,
    /// Created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

/// Wire params for [`crate::methods::put`] — local or S3 destination shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputPutParams {
    /// Local variant.
    Local(LocalPutParams),
    /// S3 variant.
    S3(PutParams),
}

/// Wire params for [`crate::methods::put_file`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputPutFileParams {
    /// Local variant.
    Local(LocalPutFileParams),
    /// S3 variant.
    S3(PutFileParams),
}

/// Wire params for [`crate::methods::get`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputGetParams {
    /// Local variant.
    Local(LocalGetParams),
    /// S3 variant.
    S3(GetParams),
}

/// Wire params for key-scoped output methods (`exists` / `probe` / `delete`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputKeyParams {
    /// Local variant.
    Local(LocalKeyParams),
    /// S3 variant.
    S3(KeyParams),
}

/// Wire params for [`crate::methods::list`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputListParams {
    /// Local variant.
    Local(LocalListParams),
    /// S3 variant.
    S3(ListParams),
}

/// Wire params for [`crate::methods::copy`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputCopyParams {
    /// Local variant.
    Local(LocalCopyParams),
    /// S3 variant.
    S3(CopyParams),
}

/// Wire params for [`crate::methods::touch_file`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputTouchFileParams {
    /// Local variant.
    Local(LocalTouchFileParams),
    /// S3 variant.
    S3(TouchFileParams),
}

/// Result of [`crate::methods::exists`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExistsResultDto {
    /// Exists.
    pub exists: bool,
}

/// Params for [`crate::methods::login_start`] — same shape as [`LoginParams`].
pub type LoginStartParams = LoginParams;

/// Params for [`crate::methods::scan_library`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanLibraryParams {
    /// Force.
    #[serde(default)]
    pub force: bool,
}

/// Params for [`crate::methods::authenticate_user`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateUserParams {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
}

/// Params for [`crate::methods::list_deals`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListDealsParams {
    /// Limit.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Params for [`crate::methods::list_accounts`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListAccountsParams {}
