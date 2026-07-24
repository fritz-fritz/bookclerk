//! ContentSource trait.

use std::path::Path;

use async_trait::async_trait;
use libation_library::LibraryStore;

use crate::error::Result;
use crate::types::{
    FetchOptions, LoginOptions, ScanOptions, ScanSummary, SourceAccount, SourceConfigOption,
    SourceFetch, SourceKind,
};

/// Pluggable audiobook store (Audible, Libro.fm, …).
///
/// Required methods cover identity, auth, scan, and fetch. Optional config
/// knobs (bitrate / container / access) are advertised via
/// [`ContentSource::config_options`] for TOML / UI discovery.
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

    /// Source-native `[sources.<id>]` knobs for config / UI discovery.
    ///
    /// Empty slice (default) means this store has no extra knobs beyond
    /// `enabled`.
    fn config_options(&self) -> &'static [SourceConfigOption] {
        &[]
    }
}
