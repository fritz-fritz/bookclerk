//! Audiobookshelf integration adapter.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use libation_config::AudiobookshelfConfig;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::client::AbsApiClient;
use crate::error::{IntegrationError, Result};
use crate::traits::{Integration, IntegrationContext};
use crate::types::{ExternalUser, IntegrationEvent, IntegrationHealth};

const PROVIDER: &str = "audiobookshelf";

/// Audiobookshelf outbound integration.
pub struct AbsIntegration {
    config: AudiobookshelfConfig,
    client: AbsApiClient,
    /// Debounce overlapping liberate→scan bursts.
    scan_lock: Mutex<()>,
    known_users: Arc<Mutex<HashSet<String>>>,
}

impl AbsIntegration {
    pub fn new(config: AudiobookshelfConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                IntegrationError::message(
                    "integrations.audiobookshelf.api_key (or LIBATION_ABS_API_KEY) is required",
                )
            })?;
        let client = AbsApiClient::new(config.base_url.clone(), api_key)?;
        Ok(Self {
            config,
            client,
            scan_lock: Mutex::new(()),
            known_users: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    #[must_use]
    pub fn client(&self) -> &AbsApiClient {
        &self.client
    }

    #[must_use]
    pub fn config(&self) -> &AudiobookshelfConfig {
        &self.config
    }

    async fn trigger_scan(&self) -> Result<()> {
        let Some(library_id) = self.config.library_id.as_deref().filter(|s| !s.is_empty()) else {
            warn!("ABS scan skipped: integrations.audiobookshelf.library_id unset");
            return Ok(());
        };
        let _guard = self.scan_lock.lock().await;
        info!(%library_id, "triggering Audiobookshelf library scan");
        self.client.scan_library(library_id, false).await
    }

    /// Trigger a library scan (optional force).
    pub async fn scan_now(&self, force: bool) -> Result<()> {
        let Some(library_id) = self.config.library_id.as_deref().filter(|s| !s.is_empty()) else {
            return Err(IntegrationError::message(
                "integrations.audiobookshelf.library_id unset",
            ));
        };
        let _guard = self.scan_lock.lock().await;
        self.client.scan_library(library_id, force).await
    }
}

#[async_trait]
impl Integration for AbsIntegration {
    fn id(&self) -> &str {
        PROVIDER
    }

    fn display_name(&self) -> &str {
        "Audiobookshelf"
    }

    async fn start(&self, ctx: IntegrationContext) -> Result<()> {
        if !self.config.watch_users {
            debug!("ABS user watch disabled");
            return Ok(());
        }
        let client = self.client.clone();
        let known = self.known_users.clone();
        let on_user = ctx.on_external_user.clone();
        // Seed + poll loop (Socket.io client deferred; poll is reliable without extra deps).
        let this_client = self.client.clone();
        let known_seed = known.clone();
        tokio::spawn(async move {
            match this_client.list_users().await {
                Ok(users) => {
                    let mut g = known_seed.lock().await;
                    for u in users {
                        g.insert(u.id);
                    }
                    info!(count = g.len(), "seeded ABS user watch set");
                }
                Err(err) => warn!(%err, "failed to seed ABS users"),
            }
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                match client.list_users().await {
                    Ok(users) => {
                        let mut g = known.lock().await;
                        for user in users {
                            if g.insert(user.id.clone()) {
                                info!(
                                    user_id = %user.id,
                                    username = %user.username,
                                    "ABS user observed"
                                );
                                let external = ExternalUser {
                                    provider: PROVIDER.into(),
                                    external_user_id: user.id,
                                    display_name: Some(user.username),
                                    access_token: None,
                                };
                                if let Some(cb) = &on_user {
                                    cb(external);
                                }
                            }
                        }
                    }
                    Err(err) => warn!(%err, "ABS user poll failed"),
                }
            }
        });
        Ok(())
    }

    async fn on_event(&self, event: &IntegrationEvent) -> Result<()> {
        match event {
            IntegrationEvent::BookLiberated { .. } => {
                if self.config.notify_scan_on_liberate {
                    self.trigger_scan().await?;
                }
                Ok(())
            }
            IntegrationEvent::ExternalUserObserved { .. } => Ok(()),
        }
    }

    async fn health(&self) -> Result<IntegrationHealth> {
        match self.client.authorize().await {
            Ok(auth) => Ok(IntegrationHealth {
                id: PROVIDER.into(),
                enabled: self.config.enabled,
                ok: true,
                detail: auth.user.map(|u| format!("authorized as {}", u.username)),
            }),
            Err(err) => Ok(IntegrationHealth {
                id: PROVIDER.into(),
                enabled: self.config.enabled,
                ok: false,
                detail: Some(err.to_string()),
            }),
        }
    }

    async fn authenticate_user(&self, username: &str, password: &str) -> Result<ExternalUser> {
        if !self.supports_credential_login() {
            return Err(IntegrationError::message(
                "integrations.audiobookshelf.allow_credential_login is false",
            ));
        }
        self.client.authenticate_user(username, password).await
    }

    fn supports_credential_login(&self) -> bool {
        self.config.allow_credential_login
    }
}
