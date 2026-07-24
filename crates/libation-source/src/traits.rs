//! ContentSource trait.

use std::path::Path;

use async_trait::async_trait;
use libation_library::LibraryStore;

use crate::error::Result;
use crate::types::{
    FetchOptions, LoginOptions, QualityLevel, ScanOptions, ScanSummary, SourceAccount, SourceFetch,
    SourceKind,
};

/// Pluggable audiobook store (Audible, Libro.fm, …).
///
/// Required methods cover identity, auth, scan, and fetch. Quality levels are
/// optional — override [`ContentSource::quality_levels`] when the store exposes
/// encode tiers (Audible High/Normal, GraphicAudio Hi/Lo, …).
#[async_trait]
pub trait ContentSource: Send + Sync {
    /// Source discriminator.
    fn kind(&self) -> SourceKind;

    /// Authenticate and persist credentials under `files_dir`.
    async fn login(&self, files_dir: &Path, opts: LoginOptions) -> Result<SourceAccount>;

    /// List accounts known to this source under `files_dir`.
    async fn list_accounts(&self, files_dir: &Path) -> Result<Vec<SourceAccount>>;

    /// Sync library rows into `library`.
    async fn scan(
        &self,
        files_dir: &Path,
        library: &LibraryStore,
        opts: ScanOptions,
    ) -> Result<ScanSummary>;

    /// Fetch everything needed to liberate one title (no storage writes).
    ///
    /// `title_id` is the source-native product id (Audible ASIN or Libro ISBN).
    async fn fetch_title(
        &self,
        files_dir: &Path,
        account_id: &str,
        title_id: &str,
        opts: &FetchOptions,
    ) -> Result<SourceFetch>;

    /// Source-native quality levels for config / UI.
    ///
    /// Empty slice (default) means this store has no quality knob.
    fn quality_levels(&self) -> &'static [QualityLevel] {
        &[]
    }
}
