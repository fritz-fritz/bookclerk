use bookclerk_config::Config;
use bookclerk_source::SourceRegistry;

/// Built-in content sources that remain linked into the host binary.
///
/// Plain storefronts (Libro.fm, Chirp, GraphicAudio) ship as external plugins
/// under `crates/bookclerk-plugins/` and are loaded via
/// [`bookclerk_plugin::load_external_sources`]. Audible stays in-process until
/// the external protocol supports Encrypted/DRM fetch.
pub fn default_registry(config: &Config) -> SourceRegistry {
    let mut registry = SourceRegistry::new();
    bookclerk_audible::register(&mut registry, config);
    registry
}

/// [`default_registry`] plus discovered external source plugins.
pub async fn default_registry_with_plugins(config: &Config) -> anyhow::Result<SourceRegistry> {
    let mut registry = default_registry(config);
    bookclerk_plugin::load_external_sources(config, &mut registry).await?;
    Ok(registry)
}

/// Resolve `--source` against registered plugin ids / aliases.
pub fn resolve_source_id(registry: &SourceRegistry, s: &str) -> anyhow::Result<String> {
    registry.resolve_id(s).ok_or_else(|| {
        let known: Vec<_> = registry
            .all()
            .into_iter()
            .map(|src| src.id().to_string())
            .collect();
        if known.is_empty() {
            anyhow::anyhow!(
                "unknown source `{s}` (no content sources registered — check `[sources.*] enabled` and plugins/)"
            )
        } else {
            anyhow::anyhow!("unknown source `{s}` (registered: {})", known.join(", "))
        }
    })
}
