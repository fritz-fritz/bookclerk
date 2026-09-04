//! Optional Cloudflare D1 library database plugin.
//!
//! Alternate `[database]` backend for remote D1. Hosts call [`open`] after
//! operator config supplies account/database credentials; see `docs/database.md`.
//! Atomic library writes use [`D1Proxy::run_typed_atomic`] rather than
//! interactive `BEGIN`.

pub mod atomic;
pub mod d1;
pub mod export_protocol;

pub use d1::{
    delete_database, ensure_database, export_sql, lookup_database, open, set_shared_proxy,
    shared_proxy, D1Proxy,
};
pub use export_protocol::{
    d1_export_poll_body, d1_export_signed_url, d1_import_upload_url, parse_d1_export_envelope,
    D1ExportPoll,
};
