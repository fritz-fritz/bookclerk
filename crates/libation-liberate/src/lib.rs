//! Liberate orchestration: license → download → decrypt → metadata → storage.

mod error;
mod naming;
mod pipeline;
mod reconcile;

pub use error::{LiberateError, Result};
pub use naming::default_storage_key;
pub use pipeline::{
    liberate_book, liberate_book_indexed, planned_storage_key, LiberateRequest, LiberateResult,
};
pub use reconcile::{
    extract_asins_from_key, find_existing_for_book, find_existing_for_request, reconcile_library,
    ReconcileOptions, ReconcileSummary, StorageIndex,
};
