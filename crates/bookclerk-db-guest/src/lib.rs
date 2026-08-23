//! Back-compat shim for `bookclerk_plugin_sdk::database_adapter`.
//!
//! New database guests should depend on `bookclerk-plugin-sdk` with feature
//! `db` and import [`bookclerk_plugin_sdk::database_adapter`] directly.

pub use bookclerk_plugin_sdk::database_adapter::*;

/// Back-compat submodule path (`bookclerk_db_guest::errors`).
pub mod errors {
    pub use bookclerk_plugin_sdk::database_adapter::errors::*;
}

/// Back-compat submodule path (`bookclerk_db_guest::migrate`).
pub mod migrate {
    pub use bookclerk_plugin_sdk::database_adapter::migrate::*;
}

/// Back-compat submodule path (`bookclerk_db_guest::session`).
pub mod session {
    pub use bookclerk_plugin_sdk::database_adapter::session::*;
}
