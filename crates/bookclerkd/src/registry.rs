//! Default content-source registry (Audible in-process + discovered plugins).

use bookclerk_config::Config;
use bookclerk_source::SourceRegistry;

/// Built-in sources linked into the daemon binary (Audible only today).
#[must_use]
pub fn default_registry(config: &Config) -> SourceRegistry {
    let mut registry = SourceRegistry::new();
    bookclerk_audible::register(&mut registry, config);
    registry
}

/// First-party sources plus dynamically discovered external plugins.
///
/// Plain storefronts (Libro / Chirp / GraphicAudio) load from `plugins/`.
pub async fn default_registry_with_plugins(config: &Config) -> anyhow::Result<SourceRegistry> {
    let mut registry = default_registry(config);
    bookclerk_plugin::load_external_sources(config, &mut registry).await?;
    Ok(registry)
}
