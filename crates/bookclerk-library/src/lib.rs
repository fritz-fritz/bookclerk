//! Canonical Bookclerk library database.
//!
//! [`LibraryStore`] is SeaORM-backed: every backend is a
//! [`sea_orm::DatabaseConnection`] proxy ([`db`]) — local `sqlite` (rusqlite)
//! by default, or Cloudflare `d1` over HTTP. The public store API stays
//! synchronous by driving each query with [`block_on_db`]. See
//! `docs/database.md`.

mod db;
mod error;
mod migrations;
mod models;
pub mod secrets;
mod store;

pub use db::{
    apply_pending_migrations, block_on_db, connect_d1, connect_from_config, connect_postgres,
    connect_sqlite, connect_sqlite_memory, resolve_d1_api_token, resolve_postgres_url, D1Proxy,
    SqliteProxy,
};
pub use error::{LibraryError, Result};
pub use models::{
    content_kind_from_classic, content_kind_to_classic, is_downloadable, is_episode,
    is_podcast_parent, portal_prefs_key, AccountLinkRecord, AccountRecord, AcquireStatus,
    BookRecord, ClaimTicketRecord, EmbeddingRecord, GlobalQueueEntry, ListeningProgressRecord,
    PortalIdentity, RequestStatus, TitleRequestRecord, UserPreferences, WorkRecord,
    OPERATOR_PREFS_KEY,
};
pub use secrets::{
    decrypt_secret, delete_secret, encrypt_secret, get_secret, list_secrets,
    migrate_accounts_dir_into_db, secret_kind, upsert_secret, EncryptedSecretRecord, SecretStore,
};
pub use store::{
    fallback_work_key, prefer_enrichment_source, wishlist_identities_match,
    CatalogEnrichmentFields, LibraryStore, NewBook, NewListeningProgress, NewTitleRequest, NewWork,
    SavedFilterRecord, UserBookFields, WishlistIdentity,
};
