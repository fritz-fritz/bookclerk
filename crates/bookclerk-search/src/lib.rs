//! Tantivy search index with classic Libation field names and query normalization.

mod engine;
mod query;

pub use engine::{SearchEngine, SearchHit};
pub use query::normalize_lucene_query;

use thiserror::Error;

/// Result type alias.
pub type Result<T> = std::result::Result<T, SearchError>;

/// Search error.
#[derive(Debug, Error)]
pub enum SearchError {
    /// Index variant.
    #[error("search index error: {0}")]
    Index(String),

    /// Query variant.
    #[error("invalid search query: {0}")]
    Query(String),

    /// Library variant.
    #[error("library error: {0}")]
    Library(#[from] bookclerk_library::LibraryError),
}

/// A saved filter expression (Bookclerk quick-filter parity).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedFilter {
    /// Name.
    pub name: String,
    /// Query.
    pub query: String,
}
