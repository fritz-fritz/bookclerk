//! Dynamic third-party plugins for Libation.
//!
//! Plugins are **separate executables** declared in `config.toml` via a
//! `command` field on `[sources.<id>]` or `[integrations.<id>]`, and spoken to
//! over newline-delimited JSON-RPC on stdio. That keeps first-party code
//! in-process while letting third parties build and distribute independently
//! (any language that can speak the protocol).
//!
//! ```toml
//! [integrations.echo]
//! enabled = true
//! command = "plugins/echo/libation-plugin-echo-integration"
//! # optional args = ["--verbose"]
//! # … opaque knobs forwarded on handshake …
//! ```
//!
//! Relative `command` paths resolve against `$LIBATION_FILES_DIR`. Bare names
//! use `PATH`. See `docs/plugins.md`.

mod discover;
mod error;
mod host;
mod manifest;
pub mod protocol;
mod rpc;

pub use discover::{discover_plugins, DiscoveredPlugin};
pub use error::{PluginError, Result};
pub use host::{
    load_external_integrations, load_external_sources, ExternalIntegration, ExternalSource,
};
pub use manifest::PluginKind;
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
        match plugin.kind {
            PluginKind::Source => {
                if !config.sources.is_enabled(&plugin.id) {
                    tracing::debug!(
                        id = %plugin.id,
                        "external source plugin disabled in config; skipping"
                    );
                    continue;
                }
                match ExternalSource::spawn(&plugin, config).await {
                    Ok(source) => {
                        tracing::info!(
                            id = %plugin.id,
                            path = %plugin.command.display(),
                            "registered external source plugin"
                        );
                        sources.register(std::sync::Arc::new(source));
                    }
                    Err(err) => {
                        tracing::warn!(
                            id = %plugin.id,
                            %err,
                            "failed to start external source plugin; skipping"
                        );
                    }
                }
            }
            PluginKind::Integration => {
                if !config.integrations.is_enabled(&plugin.id) {
                    tracing::debug!(
                        id = %plugin.id,
                        "external integration plugin disabled in config; skipping"
                    );
                    continue;
                }
                match ExternalIntegration::spawn(&plugin, config).await {
                    Ok(integration) => {
                        tracing::info!(
                            id = %plugin.id,
                            path = %plugin.command.display(),
                            "registered external integration plugin"
                        );
                        integrations.register(std::sync::Arc::new(integration));
                    }
                    Err(err) => {
                        tracing::warn!(
                            id = %plugin.id,
                            %err,
                            "failed to start external integration plugin; skipping"
                        );
                    }
                }
            }
            PluginKind::Output => {
                tracing::warn!(
                    id = %plugin.id,
                    "output plugins are discovered but not yet loaded (coming soon)"
                );
            }
        }
    }
    Ok(())
}
