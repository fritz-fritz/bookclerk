//! Registry of content sources.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use libation_library::LibraryStore;

use crate::error::{Result, SourceError};
use crate::traits::ContentSource;
use crate::types::{ScanOptions, ScanSummary, SourceKind};

/// Maps [`SourceKind`] → installed [`ContentSource`] implementations.
#[derive(Clone, Default)]
pub struct SourceRegistry {
    sources: HashMap<SourceKind, Arc<dyn ContentSource>>,
}

impl SourceRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a source implementation.
    pub fn register(&mut self, source: Arc<dyn ContentSource>) {
        self.sources.insert(source.kind(), source);
    }

    /// Look up a source by kind.
    #[must_use]
    pub fn get(&self, kind: SourceKind) -> Option<Arc<dyn ContentSource>> {
        self.sources.get(&kind).cloned()
    }

    /// Require a source or return an error.
    pub fn require(&self, kind: SourceKind) -> Result<Arc<dyn ContentSource>> {
        self.get(kind)
            .ok_or_else(|| SourceError::api(format!("content source `{kind}` is not registered")))
    }

    /// All registered sources in stable order (Audible first, then Libro).
    #[must_use]
    pub fn all(&self) -> Vec<Arc<dyn ContentSource>> {
        let mut kinds: Vec<SourceKind> = self.sources.keys().copied().collect();
        kinds.sort_by_key(|k| match k {
            SourceKind::Audible => 0,
            SourceKind::LibroFm => 1,
        });
        kinds
            .into_iter()
            .filter_map(|k| self.sources.get(&k).cloned())
            .collect()
    }

    /// Scan every registered source (honoring per-source account filters).
    pub async fn scan_all(
        &self,
        files_dir: &Path,
        library: &LibraryStore,
        opts: ScanOptions,
    ) -> Result<ScanSummary> {
        let mut total = ScanSummary::default();
        let mut any = false;
        for source in self.all() {
            match source.scan(files_dir, library, opts.clone()).await {
                Ok(summary) => {
                    any = true;
                    total.merge(&summary);
                }
                Err(SourceError::NoAccounts(msg)) => {
                    tracing::debug!(
                        source = %source.kind(),
                        %msg,
                        "skipping source with no accounts"
                    );
                }
                Err(err) => return Err(err),
            }
        }
        if !any && total.accounts == 0 {
            return Err(SourceError::no_accounts(
                "no accounts configured — run `libation auth login` first",
            ));
        }
        Ok(total)
    }
}
