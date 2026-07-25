//! Default content-source registry (Audible, Libro.fm, GraphicAudio, Chirp).

use libation_config::Config;
use libation_source::SourceRegistry;

/// Build a registry with enabled first-party content sources from config.
#[must_use]
pub fn default_registry(config: &Config) -> SourceRegistry {
    let mut registry = SourceRegistry::new();
    libation_audible::register(&mut registry, config);
    libation_libro::register(&mut registry, config);
    libation_graphicaudio::register(&mut registry, config);
    libation_chirp::register(&mut registry, config);
    registry
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
                "unknown source `{s}` (no content sources registered — check `[sources.*] enabled`)"
            )
        } else {
            anyhow::anyhow!("unknown source `{s}` (registered: {})", known.join(", "))
        }
    })
}

/// Credential filename suffixes from registered plugins.
///
/// When every source is disabled, still collects suffixes from all first-party
/// plugins so revoke can clean up leftover auth files.
#[must_use]
pub fn auth_credential_suffixes(config: &Config) -> Vec<&'static str> {
    let registry = default_registry(config);
    let suffixes = registry.all_auth_credential_suffixes();
    if !suffixes.is_empty() {
        return suffixes;
    }
    let mut all = SourceRegistry::new();
    all.register(std::sync::Arc::new(libation_audible::from_config(config)));
    all.register(std::sync::Arc::new(libation_libro::from_config(config)));
    all.register(std::sync::Arc::new(libation_graphicaudio::from_config(
        config,
    )));
    all.register(std::sync::Arc::new(libation_chirp::from_config(config)));
    all.all_auth_credential_suffixes()
}
