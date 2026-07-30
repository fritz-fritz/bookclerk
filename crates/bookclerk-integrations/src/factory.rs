//! Build integration registry from config.

use std::sync::Arc;

use bookclerk_config::Config;
use tracing::info;

use crate::abs::AbsIntegration;
use crate::error::Result;
use crate::registry::IntegrationRegistry;

/// Register first-party integrations into an existing registry.
///
/// Prefer [`bookclerk_plugin::register_builtin_integrations`] from hosts so
/// binaries do not name store crates. When ABS is enabled but misconfigured
/// (e.g. missing API key), it is still registered so health/diagnose surface
/// the error instead of silently omitting the adapter.
pub fn register_builtins(config: &Config, registry: &mut IntegrationRegistry) -> Result<()> {
    let abs = config.integrations.audiobookshelf();
    if abs.enabled {
        info!("enabling audiobookshelf integration");
        registry.register(Arc::new(AbsIntegration::from_config(abs)));
    }
    Ok(())
}

/// Construct a registry with only first-party integrations from config.
///
/// Hosts that also load external plugins should prefer
/// [`bookclerk_plugin::load_integrations`].
pub fn from_config(config: &Config) -> Result<IntegrationRegistry> {
    let mut registry = IntegrationRegistry::new();
    register_builtins(config, &mut registry)?;
    Ok(registry)
}
