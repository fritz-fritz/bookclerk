//! Pluggable outbound integration trait.

use async_trait::async_trait;

use crate::error::Result;
use crate::types::{ExternalUser, IntegrationEvent, IntegrationHealth};

/// Context passed when starting background watchers.
#[derive(Clone, Default)]
pub struct IntegrationContext {
    /// Optional callback when an external user is observed (claim minting).
    pub on_external_user: Option<std::sync::Arc<dyn Fn(ExternalUser) + Send + Sync>>,
}

/// Downstream consumer of liberated books / identity signals (e.g. Audiobookshelf).
#[async_trait]
pub trait Integration: Send + Sync {
    /// Stable integration id (`audiobookshelf`, …).
    fn id(&self) -> &str;

    /// Start background tasks (user watchers, etc.).
    async fn start(&self, ctx: IntegrationContext) -> Result<()>;

    /// Handle a fan-out event (best-effort; errors are logged by the registry).
    async fn on_event(&self, event: &IntegrationEvent) -> Result<()>;

    /// Connectivity / config health check.
    async fn health(&self) -> Result<IntegrationHealth>;

    /// Validate end-user credentials against this integration (portal return login).
    async fn authenticate_user(&self, username: &str, password: &str) -> Result<ExternalUser>;
}
