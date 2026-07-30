//! In-process registration of first-party plugins.
//!
//! Hosts (`bookclerk` / `bookclerkd`) call these helpers instead of naming
//! store crates. That keeps binaries store-agnostic while still allowing
//! `cargo run` without staging external plugin binaries — the library crates
//! are linked through this host crate and [`register`](register_builtin_sources)
//! installs them into the same registries external plugins use.
//!
//! External plugins under `crates/bookclerk-plugins/` remain the distribution
//! form; [`crate::load_external_sources`] / [`crate::load_external_integrations`]
//! skip ids already registered here.

use bookclerk_config::Config;
use bookclerk_integrations::IntegrationRegistry;
use bookclerk_source::SourceRegistry;

/// Register first-party content sources in-process (Audible, Libro.fm, Chirp,
/// GraphicAudio) when enabled in config.
pub fn register_builtin_sources(config: &Config, registry: &mut SourceRegistry) {
    bookclerk_audible::register(registry, config);
    bookclerk_libro::register(registry, config);
    bookclerk_graphicaudio::register(registry, config);
    bookclerk_chirp::register(registry, config);
}

/// Register first-party integrations in-process (Audiobookshelf) when enabled.
///
/// Misconfigured-but-enabled ABS is still registered so health/diagnose surface
/// the error (same behavior as [`bookclerk_integrations::from_config`]).
pub fn register_builtin_integrations(
    config: &Config,
    registry: &mut IntegrationRegistry,
) -> crate::Result<()> {
    bookclerk_integrations::register_builtins(config, registry)
        .map_err(|e| crate::PluginError::message(e.to_string()))
}

/// Built-in sources plus discovered external source plugins.
pub async fn load_sources(config: &Config) -> crate::Result<SourceRegistry> {
    let mut registry = SourceRegistry::new();
    register_builtin_sources(config, &mut registry);
    crate::load_external_sources(config, &mut registry).await?;
    Ok(registry)
}

/// Built-in integrations plus discovered external integration plugins.
pub async fn load_integrations(config: &Config) -> crate::Result<IntegrationRegistry> {
    let mut registry = IntegrationRegistry::new();
    register_builtin_integrations(config, &mut registry)?;
    crate::load_external_integrations(config, &mut registry).await?;
    Ok(registry)
}
