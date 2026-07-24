//! [`GraphicAudioSource`]: [`ContentSource`] implementation for GraphicAudio.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use libation_library::LibraryStore;
use libation_source::{
    ContentSource, FetchOptions, LoginOptions, ScanOptions, ScanSummary, SourceAccount,
    SourceFetch, SourceKind,
};

use crate::auth::{
    auth_file_for_account, ensure_accounts_dir, find_auth_file, list_auth_files, load_auth,
    save_auth, GraphicAudioAuthFile,
};
use crate::client::{GraphicAudioClient, DEFAULT_BASE_URL};
use crate::download::fetch_title_materials_with_quality;
use crate::error::{GraphicAudioError, Result};
use crate::sync::{scan_library, ScanOptions as GaScanOptions};

/// GraphicAudio content source.
#[derive(Debug, Clone)]
pub struct GraphicAudioSource {
    base_url: String,
}

impl Default for GraphicAudioSource {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphicAudioSource {
    /// Production GraphicAudio API origin.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Override API base (wiremock / staging).
    #[must_use]
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    /// Arc-wrapped instance for [`libation_source::SourceRegistry`].
    #[must_use]
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Login and persist `.ga.auth`.
    pub async fn login_account(
        &self,
        files_dir: &Path,
        opts: LoginOptions,
    ) -> Result<SourceAccount> {
        let email = opts
            .email
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| GraphicAudioError::auth("GraphicAudio login requires email"))?;
        let password = opts
            .password
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| GraphicAudioError::auth("GraphicAudio login requires password"))?;

        ensure_accounts_dir(files_dir)?;
        let path = auth_file_for_account(files_dir, opts.label.as_deref(), email);
        if path.is_file() && !opts.force {
            let existing = load_auth(&path)?;
            return Ok(source_account_from_auth(&existing));
        }

        let client_id = if path.is_file() {
            load_auth(&path)
                .map(|a| a.client_id)
                .unwrap_or_else(|_| format!("libation-{}", uuid::Uuid::new_v4()))
        } else {
            format!("libation-{}", uuid::Uuid::new_v4())
        };

        let mut client = GraphicAudioClient::new(&self.base_url);
        let token = client.login(email, password, &client_id).await?;

        let marketplace = if opts.marketplace.trim().is_empty() {
            String::from("us")
        } else {
            opts.marketplace.trim().to_ascii_lowercase()
        };

        let auth = GraphicAudioAuthFile {
            token,
            client_id,
            email: email.to_string(),
            marketplace,
            label: opts.label.clone(),
        };
        save_auth(&path, &auth)?;

        tracing::info!(
            email = %auth.email,
            path = %path.display(),
            "saved GraphicAudio auth file"
        );

        Ok(source_account_from_auth(&auth))
    }
}

#[async_trait]
impl ContentSource for GraphicAudioSource {
    fn kind(&self) -> SourceKind {
        SourceKind::GraphicAudio
    }

    async fn login(
        &self,
        files_dir: &Path,
        opts: LoginOptions,
    ) -> libation_source::Result<SourceAccount> {
        self.login_account(files_dir, opts)
            .await
            .map_err(Into::into)
    }

    async fn list_accounts(&self, files_dir: &Path) -> libation_source::Result<Vec<SourceAccount>> {
        let mut out = Vec::new();
        for path in list_auth_files(files_dir)? {
            match load_auth(&path) {
                Ok(auth) => out.push(source_account_from_auth(&auth)),
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "skipping unreadable GraphicAudio auth file"
                    );
                }
            }
        }
        Ok(out)
    }

    async fn scan(
        &self,
        files_dir: &Path,
        library: &LibraryStore,
        opts: ScanOptions,
    ) -> libation_source::Result<ScanSummary> {
        scan_library(
            files_dir,
            library,
            GaScanOptions::from(&opts),
            Some(self.base_url.as_str()),
        )
        .await
        .map_err(Into::into)
    }

    async fn fetch_title(
        &self,
        files_dir: &Path,
        account_id: &str,
        title_id: &str,
        opts: &FetchOptions,
    ) -> libation_source::Result<SourceFetch> {
        let path = find_auth_file(files_dir, account_id)?;
        let auth = load_auth(&path)?;
        let client = GraphicAudioClient::new(&self.base_url).with_token(&auth.token);
        let prefer_hi = opts
            .download
            .ingest_quality("graphicaudio")
            .prefers_graphicaudio_hi();
        let plain =
            fetch_title_materials_with_quality(&client, title_id, &opts.cache_dir, prefer_hi)
                .await?;
        Ok(SourceFetch::Plain(plain))
    }
}

fn source_account_from_auth(auth: &GraphicAudioAuthFile) -> SourceAccount {
    SourceAccount {
        account_id: auth.account_id().to_string(),
        source: SourceKind::GraphicAudio,
        marketplace: auth.marketplace.clone(),
        label: auth.label.clone().or_else(|| Some(auth.email.clone())),
        scan_enabled: true,
    }
}
