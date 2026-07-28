//! Canonical Bookclerk library database.
//!
//! Local default is SQLite via rusqlite. Database **plugins** (SeaORM connection
//! factory for `sqlite` / Cloudflare `d1`) live in [`db`]. See `docs/database.md`.

mod db;
mod error;
mod migrations;
mod models;
mod store;

pub use db::{
    block_on_db, connect_d1, connect_from_config, connect_sqlite, resolve_d1_api_token, D1Proxy,
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
pub use store::{
    fallback_work_key, prefer_enrichment_source, wishlist_identities_match,
    CatalogEnrichmentFields, LibraryStore, NewBook, NewListeningProgress, NewTitleRequest, NewWork,
    SavedFilterRecord, UserBookFields, WishlistIdentity,
};
