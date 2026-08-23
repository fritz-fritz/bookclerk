//! Shared helpers for first-party database plugin guests.
//!
//! Engine-specific connect/proxy code lives in each guest crate; this crate
//! owns the per-process SeaORM session (ping/query/execute/begin) and generic
//! SQL-string execution. Hosts select schema versions.

pub mod errors;
pub mod migrate;
pub mod session;

pub use errors::{plugin_error_from_db_err, plugin_error_from_engine};
pub use migrate::execute_sql_scripts;
pub use session::{
    guest_atomic, guest_begin, guest_capabilities, guest_commit, guest_execute,
    guest_execute_atomic, guest_execute_atomic_on_txn, guest_ping, guest_query, guest_query_page,
    guest_rollback, row_to_dto, set_connection,
};
