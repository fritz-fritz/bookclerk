//! Dynamic third-party plugins for Libation.
//!
//! Plugins are **separate executables** discovered from plugin directories and
//! spoken to over newline-delimited JSON-RPC on stdio. That keeps first-party
//! code in-process while letting third parties build and distribute
//! independently (any language that can speak the protocol).
//!
//! # Layout
//!
//! ```text
//! $LIBATION_FILES_DIR/plugins/
//!   my-plugin/
//!     plugin.toml
//!     my-plugin          # executable (or `command` in the manifest)
//! ```
//!
//! Additional search roots: `LIBATION_PLUGIN_DIRS` (path-list, OS-separated).
//!
//! See `docs/plugins.md` for the protocol and manifest schema.

mod discover;
mod error;
mod host;
mod manifest;
pub mod protocol;
mod rpc;

pub use discover::{discover_plugins, plugin_search_dirs, DiscoveredPlugin};
pub use error::{PluginError, Result};
pub use host::{
    load_external_integrations, load_external_sources, ExternalIntegration, ExternalSource,
};
pub use manifest::{PluginKind, PluginManifest};
pub use protocol::{methods, HandshakeResult, HealthDto, PLUGIN_API_VERSION};
pub use rpc::{PluginClient, PluginGuest};

/// Register discovered external plugins into the in-process registries.
pub async fn register_discovered(
    config: &libation_config::Config,
    sources: &mut libation_source::SourceRegistry,
    integrations: &mut libation_integrations::IntegrationRegistry,
) -> Result<()> {
    let plugins = discover_plugins(config)?;
    for plugin in plugins {
        match plugin.manifest.kind {
            PluginKind::Source => {
                if !config.sources.is_enabled(&plugin.manifest.id) {
                    tracing::debug!(
                        id = %plugin.manifest.id,
                        "external source plugin disabled in config; skipping"
                    );
                    continue;
                }
                match ExternalSource::spawn(&plugin, config).await {
                    Ok(source) => {
                        tracing::info!(
                            id = %plugin.manifest.id,
                            path = %plugin.command.display(),
                            "registered external source plugin"
                        );
                        sources.register(std::sync::Arc::new(source));
                    }
                    Err(err) => {
                        tracing::warn!(
                            id = %plugin.manifest.id,
                            %err,
                            "failed to start external source plugin; skipping"
                        );
                    }
                }
            }
            PluginKind::Integration => {
                if !config.integrations.is_enabled(&plugin.manifest.id) {
                    tracing::debug!(
                        id = %plugin.manifest.id,
                        "external integration plugin disabled in config; skipping"
                    );
                    continue;
                }
                match ExternalIntegration::spawn(&plugin, config).await {
                    Ok(integration) => {
                        tracing::info!(
                            id = %plugin.manifest.id,
                            path = %plugin.command.display(),
                            "registered external integration plugin"
                        );
                        integrations.register(std::sync::Arc::new(integration));
                    }
                    Err(err) => {
                        tracing::warn!(
                            id = %plugin.manifest.id,
                            %err,
                            "failed to start external integration plugin; skipping"
                        );
                    }
                }
            }
            PluginKind::Output => {
                tracing::warn!(
                    id = %plugin.manifest.id,
                    "output plugins are discovered but not yet loaded (coming soon)"
                );
            }
        }
    }
    Ok(())
}
