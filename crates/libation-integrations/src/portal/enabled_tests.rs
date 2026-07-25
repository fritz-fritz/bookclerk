//! Portal appearance / actions follow plugin `enabled` flags.

use libation_config::Config;
use libation_source::SourceKind;

use super::html::{credential_login_brands, landing_page};

#[test]
fn sources_is_enabled_matches_plugin_tables() {
    let mut cfg = Config::default();
    assert!(cfg.sources.is_enabled("audible"));
    assert!(cfg.sources.is_enabled("libro"));
    cfg.sources.chirp.enabled = false;
    cfg.sources.graphicaudio.enabled = false;
    assert!(!cfg.sources.is_enabled("chirp"));
    assert!(!cfg.sources.is_enabled("graphicaudio"));
    assert!(cfg.sources.is_enabled("audible"));
}

#[test]
fn integrations_is_enabled_defaults_off_for_abs() {
    let mut cfg = Config::default();
    assert!(!cfg.integrations.is_enabled("audiobookshelf"));
    assert!(!cfg.integrations.is_enabled("abs"));
    cfg.integrations.audiobookshelf.enabled = true;
    assert!(cfg.integrations.is_enabled("audiobookshelf"));
    assert!(!cfg.integrations.is_enabled("unknown"));
}

#[test]
fn landing_page_embeds_only_enabled_source_brands() {
    let enabled = [SourceKind::Audible, SourceKind::LibroFm];
    let html = landing_page("/connect", &[], &enabled);
    assert!(html.contains("\"audible\""));
    assert!(html.contains("\"libro\""));
    assert!(!html.contains("\"chirp\""));
    assert!(!html.contains("\"graphicaudio\""));
    assert!(!html.contains("\"audiobookshelf\""));
}

#[test]
fn landing_page_embeds_credential_provider_when_enabled() {
    let providers = credential_login_brands(&[String::from("audiobookshelf")]);
    assert_eq!(providers.len(), 1);
    let html = landing_page("/connect", &providers, &[SourceKind::Audible]);
    assert!(html.contains("Sign in with Audiobookshelf") || html.contains("audiobookshelf"));
    assert!(html.contains("\"audiobookshelf\""));
    assert!(!html.contains("\"chirp\""));
}
