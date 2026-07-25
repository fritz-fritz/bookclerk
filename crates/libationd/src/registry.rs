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
