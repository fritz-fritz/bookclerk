//! Canonical Libation library database (SQLite via sqlx).

mod error;
mod models;
mod store;

pub use error::{LibraryError, Result};
pub use models::{AccountRecord, BookRecord, LiberateStatus};
pub use store::{LibraryStore, NewBook};
