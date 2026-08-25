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
//! | Engine error → structured [`crate::PluginError`] | [`plugin_error_from_engine`], [`plugin_error_from_db_err`] |
//! | Host-provided SQL scripts (no Bookclerk migrations) | [`execute_sql_scripts`] |
//! | Typed SQL `NULL` for proxy row decode | [`typed_null`] |
//!
//! First-party database guests enable `host-db-session` for SeaORM session
//! workers (`guest_execute_atomic`, `set_connection`, …). Those helpers are not
//! part of the stable third-party author API.

pub mod errors;
#[cfg(feature = "host-db-session")]
pub mod host_session;
pub mod migrate;
#[cfg(feature = "host-db-session")]
pub mod session;
#[cfg(feature = "host-db-session")]
pub mod sql;

#[cfg(test)]
mod public_surface;

pub use errors::{plugin_error_from_db_err, plugin_error_from_engine};
#[cfg(feature = "host-db-session")]
pub use host_session::{host_session, GuestHostAdapterSession};
pub use migrate::{execute_sql_scripts, split_sql_statements, typed_null};
#[cfg(feature = "host-db-session")]
pub use session::{
    guest_bootstrap, guest_capabilities, guest_execute_atomic, guest_ping, set_connection,
};
#[cfg(feature = "host-db-session")]
pub use sql::{guest_sql, GuestStatement};
