//! Build integration registry from config.

use std::sync::Arc;

use libation_config::Config;
use tracing::info;

use crate::abs::AbsIntegration;
use crate::error::Result;
use crate::registry::IntegrationRegistry;

/// Construct enabled integrations from config.
pub fn from_config(config: &Config) -> Result<IntegrationRegistry> {
    let mut registry = IntegrationRegistry::new();
    let abs = &config.integrations.audiobookshelf;
    if abs.enabled {
        match AbsIntegration::new(abs.clone()) {
            Ok(integration) => {
                info!("enabling audiobookshelf integration");
                registry.register(Arc::new(integration));
            }
            Err(err) => {
                tracing::warn!(%err, "audiobookshelf integration enabled but misconfigured; skipping");
            }
        }
    }
    Ok(registry)
}
