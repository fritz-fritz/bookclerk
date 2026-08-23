//! Database adapter helpers for third-party database plugin authors.
//!
//! # Audience
//!
//! Rust authors building a **database** guest (`kind = "database"`). Use this
//! module with the `db` feature enabled on `bookclerk-plugin-sdk`:
//!
//! ```toml
//! [dependencies]
//! bookclerk-plugin-sdk = { version = "0.1", features = ["db"] }
//! ```
//!
//! # What to use
//!
//! | Need | Entry point |
//! | --- | --- |
//! | Per-process SeaORM session (ping/query/execute/begin/atomic) | [`set_connection`], [`guest_execute_atomic`], … |
//! | Engine error → structured [`crate::PluginError`] | [`plugin_error_from_engine`], [`plugin_error_from_db_err`] |
//! | Host-provided SQL scripts (no Bookclerk migrations) | [`execute_sql_scripts`] |
//! | Typed SQL `NULL` for proxy row decode | [`typed_null`] |
//! | SeaORM ↔ wire DTO helpers | crate-root `db` re-exports ([`crate::StatementDto`], [`crate::proxy_rows_to_dto`], …) |
//!
//! Engine-specific connect/proxy code stays in your guest crate; this module
//! owns the shared session worker and generic SQL-string execution. Hosts
//! select schema versions and author execution plans.
//!
//! # Example
//!
//! ```ignore
//! use bookclerk_plugin_sdk::database_adapter::{
//!     guest_capabilities, guest_execute_atomic, plugin_error_from_engine, set_connection,
//! };
//! ```

pub mod errors;
pub mod migrate;
pub mod session;

pub use errors::{plugin_error_from_db_err, plugin_error_from_engine};
pub use migrate::{execute_sql_scripts, split_sql_statements, typed_null};
pub use session::{
    guest_atomic, guest_begin, guest_capabilities, guest_commit, guest_execute,
    guest_execute_atomic, guest_execute_atomic_on_txn, guest_ping, guest_query, guest_query_page,
    guest_rollback, row_to_dto, set_connection,
};
