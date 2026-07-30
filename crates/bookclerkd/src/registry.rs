use bookclerk_config::Config;
use bookclerk_source::SourceRegistry;

/// Built-in content sources linked into the host for an easy `cargo run` path.
///
/// First-party sources also ship as external plugins under
/// `crates/bookclerk-plugins/`. [`default_registry_with_plugins`] loads those
/// after `register()`; duplicate ids are skipped so in-process wins.
pub fn default_registry(config: &Config) -> SourceRegistry {
    let mut registry = SourceRegistry::new();
    bookclerk_audible::register(&mut registry, config);
    bookclerk_libro::register(&mut registry, config);
    bookclerk_graphicaudio::register(&mut registry, config);
    bookclerk_chirp::register(&mut registry, config);
    registry
}

/// [`default_registry`] plus discovered external source plugins.
pub async fn default_registry_with_plugins(config: &Config) -> anyhow::Result<SourceRegistry> {
    let mut registry = default_registry(config);
    bookclerk_plugin::load_external_sources(config, &mut registry).await?;
    Ok(registry)
}
