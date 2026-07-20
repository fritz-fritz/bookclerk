//! Liberate orchestration: license → download → decrypt → metadata → storage.

mod convert;
mod error;
mod cue;
mod naming;
mod pipeline;
mod reconcile;

pub use convert::{convert_book, ConvertRequest, ConvertSummary};
pub use error::{LiberateError, Result};
pub use naming::{
    audio_basename, default_storage_key, sidecar_key, storage_key, swap_audio_extension,
    NamingContext,
};
pub use pipeline::{
    liberate_book, liberate_book_indexed, liberate_pdf_only, planned_storage_key, LiberateRequest,
    LiberateResult,
};
pub use reconcile::{
    extract_asins_from_key, find_existing_for_book, find_existing_for_request, reconcile_library,
    ReconcileOptions, ReconcileSummary, StorageIndex,
};
