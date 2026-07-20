//! Search crate placeholder (Tantivy lands in Phase 4).

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, SearchError>;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search is not implemented yet (Phase 4)")]
    NotImplemented,
}

/// A saved filter expression (Libation parity).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedFilter {
    pub name: String,
    pub query: String,
}

/// Search service stub.
#[derive(Debug, Default)]
pub struct SearchService;

impl SearchService {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Run a query against the library index.
    pub fn search(&self, _query: &str) -> Result<Vec<String>> {
        Err(SearchError::NotImplemented)
    }
}
