//! Liberate orchestration: license → download → decrypt → metadata → storage.

mod error;
mod naming;
mod pipeline;

pub use error::{LiberateError, Result};
pub use naming::default_storage_key;
pub use pipeline::{liberate_book, planned_storage_key, LiberateRequest, LiberateResult};
