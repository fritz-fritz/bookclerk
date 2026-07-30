//! [`ChirpSource`]: [`ContentSource`] implementation for Chirp.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bookclerk_config::Config;
use bookclerk_library::LibraryStore;
use bookclerk_source::{
    ContentSource, FetchOptions, LoginOptions, PortalAuthMode, ScanOptions, ScanSummary,
    SourceAccount, SourceBrand, SourceFetch, SourceRegistry,
};

use crate::auth::ChirpAuthFile;
use crate::client::{ChirpClient, DEFAULT_GRAPHQL_URL};
use crate::db::{delete_auth_from_db, list_auth_from_db, load_auth_from_db, save_auth_to_db};
use crate::download::fetch_title_materials;
use crate::error::{ChirpError, Result};
use crate::sync::{scan_library, ScanOptions as ChirpScanOptions};

/// Canonical plugin id.
pub const ID: &str = "chirp";

/// Env var for non-interactive password login.
pub const PASSWORD_ENV: &str = "BOOKCLERK_CHIRP_PASSWORD";

type AuthCache = Arc<Mutex<HashMap<String, ChirpAuthFile>>>;

fn empty_auth_cache() -> AuthCache {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Chirp content source.
#[derive(Debug, Clone)]
pub struct ChirpSource {
    graphql_url: String,
    auth_cache: AuthCache,
}

impl Default for ChirpSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ChirpSource {
    #[must_use]
    pub fn new() -> Self {
        Self {
            graphql_url: DEFAULT_GRAPHQL_URL.to_string(),
            auth_cache: empty_auth_cache(),
        }
    }

    /// Parse `[sources.chirp]` (enable flag only today).
    #[must_use]
    pub fn from_config(_config: &Config) -> Self {
        Self::new()
    }

    #[must_use]
    pub fn with_graphql_url(graphql_url: impl Into<String>) -> Self {
        Self {
            graphql_url: graphql_url.into(),
            auth_cache: empty_auth_cache(),
        }
    }

    #[must_use]
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    fn cache_put(&self, account_id: &str, auth: &ChirpAuthFile) {
        if let Ok(mut guard) = self.auth_cache.lock() {
            guard.insert(account_id.to_string(), auth.clone());
        }
    }

    fn cache_remove(&self, account_id: &str) {
        if let Ok(mut guard) = self.auth_cache.lock() {
            guard.remove(account_id);
        }
    }

    async fn auth_for_account(
        &self,
        library: &LibraryStore,
        account_id: &str,
    ) -> bookclerk_source::Result<ChirpAuthFile> {
        if let Ok(guard) = self.auth_cache.lock() {
            if let Some(auth) = guard.get(account_id) {
                return Ok(auth.clone());
            }
        }
        let auth = load_auth_from_db(library, account_id)
            .await
            .map_err(|e| bookclerk_source::SourceError::Auth(e.to_string()))?
            .ok_or_else(|| {
                bookclerk_source::SourceError::Auth(format!(
                    "no Chirp credentials for account `{account_id}` in DB"
                ))
            })?;
        self.cache_put(account_id, &auth);
        Ok(auth)
    }

    /// Login and persist credentials to DB.
    pub async fn login_account(
        &self,
        library: &LibraryStore,
        opts: LoginOptions,
    ) -> Result<SourceAccount> {
        let email = opts
            .email
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ChirpError::auth("Chirp login requires email"))?;
        let password = opts
            .password
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ChirpError::auth("Chirp login requires password"))?;

        let mut client = ChirpClient::new(&self.graphql_url);
        let user = client.login(email, password).await?;

        let marketplace = if opts.marketplace.trim().is_empty() {
            String::from("us")
        } else {
            opts.marketplace.trim().to_ascii_lowercase()
        };

        let auth = ChirpAuthFile {
            access_token: user.token,
            web_token: user.web_token,
            email: user.email,
            user_id: Some(user.id),
            marketplace,
            label: opts.label.clone(),
        };

        let account_id = auth.account_id().to_string();
        save_auth_to_db(&auth, library, &account_id)
            .await
            .map_err(|e| ChirpError::auth(format!("failed to save Chirp auth: {e}")))?;
        self.cache_put(&account_id, &auth);

        tracing::info!(
            email = %auth.email,
            user_id = ?auth.user_id,
            "saved Chirp auth to encrypted_secrets"
        );

        Ok(source_account_from_auth(&auth))
    }

    /// Delete a Chirp account from the DB.
    pub async fn delete_account(&self, library: &LibraryStore, account_id: &str) -> Result<()> {
        self.cache_remove(account_id);
        delete_auth_from_db(library, account_id).await
    }
}

#[async_trait]
impl ContentSource for ChirpSource {
    fn id(&self) -> &str {
        ID
    }

    fn display_name(&self) -> &str {
        "Chirp"
    }

    fn portal_auth_mode(&self) -> PortalAuthMode {
        PortalAuthMode::Password
    }

    fn portal_brand(&self) -> SourceBrand {
        SourceBrand {
            id: "chirp",
            name: "Chirp",
            bg: "#0F766E",
            fg: "#ECFEFF",
            accent: "#14B8A6",
            icon_url: "https://www.google.com/s2/favicons?domain=chirpbooks.com&sz=128",
        }
    }

    fn password_env_var(&self) -> Option<&'static str> {
        Some(PASSWORD_ENV)
    }

    fn sort_key(&self) -> u32 {
        3
    }

    async fn login(
        &self,
        library: &LibraryStore,
        opts: LoginOptions,
    ) -> bookclerk_source::Result<SourceAccount> {
        self.login_account(library, opts).await.map_err(Into::into)
    }

    async fn list_accounts(
        &self,
        library: &LibraryStore,
    ) -> bookclerk_source::Result<Vec<SourceAccount>> {
        let records = list_auth_from_db(library)
            .await
            .map_err(Into::<bookclerk_source::SourceError>::into)?;
        Ok(records
            .into_iter()
            .map(|(_id, auth)| source_account_from_auth(&auth))
            .collect())
    }

    async fn scan(
        &self,
        library: &LibraryStore,
        opts: ScanOptions,
    ) -> bookclerk_source::Result<ScanSummary> {
        scan_library(
            library,
            ChirpScanOptions::from(&opts),
            Some(self.graphql_url.as_str()),
        )
        .await
        .map_err(Into::into)
    }

    async fn fetch_title(
        &self,
        library: &LibraryStore,
        account_id: &str,
        title_id: &str,
        opts: &FetchOptions,
    ) -> bookclerk_source::Result<SourceFetch> {
        let auth = self.auth_for_account(library, account_id).await?;
        let _ = &opts.files_dir;
        let client = ChirpClient::new(&self.graphql_url).with_token(&auth.access_token);
        let plain = fetch_title_materials(&client, title_id, &opts.cache_dir).await?;
        Ok(SourceFetch::Plain(plain))
    }
}

fn source_account_from_auth(auth: &ChirpAuthFile) -> SourceAccount {
    SourceAccount {
        account_id: auth.account_id().to_string(),
        source: ID.into(),
        marketplace: auth.marketplace.clone(),
        label: auth.label.clone().or_else(|| Some(auth.email.clone())),
        scan_enabled: true,
    }
}

/// Parse `[sources.chirp]` into a [`ChirpSource`].
#[must_use]
pub fn from_config(config: &Config) -> ChirpSource {
    ChirpSource::from_config(config)
}

/// Register Chirp when `[sources.chirp] enabled` (default true).
pub fn register(registry: &mut SourceRegistry, config: &Config) {
    if config.sources.is_enabled(ID) {
        registry.register(Arc::new(from_config(config)));
    }
}
