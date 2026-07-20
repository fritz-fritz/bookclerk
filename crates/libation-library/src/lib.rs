//! Canonical Libation library database (SQLite via rusqlite).

mod error;
mod migrations;
mod models;
mod store;

pub use error::{LibraryError, Result};
pub use models::{AccountRecord, BookRecord, LiberateStatus};
pub use store::{LibraryStore, NewBook, SavedFilterRecord, UserBookFields};
