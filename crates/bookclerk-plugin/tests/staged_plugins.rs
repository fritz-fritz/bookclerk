//! Host↔guest handshake against staged first-party plugin artifacts.
//!
//! Requires binaries under `BOOKCLERK_PLUGIN_ARTIFACTS` (CI stages them via
//! `scripts/stage-first-party-plugins.sh`). When unset, the test is skipped.

use std::path::PathBuf;

use bookclerk_config::{Config, Paths};
use bookclerk_plugin::{discover_plugins, methods, HealthDto, PluginClient, PluginKind};

fn artifacts_dir() -> Option<PathBuf> {
    std::env::var_os("BOOKCLERK_PLUGIN_ARTIFACTS").map(PathBuf::from)
}

#[tokio::test]
async fn staged_first_party_plugins_handshake() {
    let Some(artifacts) = artifacts_dir() else {
        eprintln!("skipping: set BOOKCLERK_PLUGIN_ARTIFACTS after stage-first-party-plugins.sh");
        return;
    };
    assert!(
        artifacts.is_dir(),
        "BOOKCLERK_PLUGIN_ARTIFACTS is not a directory: {}",
        artifacts.display()
    );

    let files = tempfile::tempdir().expect("temp files dir");
    // Point discovery at the staged tree via BOOKCLERK_PLUGIN_DIRS.
    std::env::set_var("BOOKCLERK_PLUGIN_DIRS", artifacts.as_os_str());
    let config = Config {
        paths: Some(Paths::from_files_dir(files.path().to_path_buf())),
        ..Default::default()
    };

    let plugins = discover_plugins(&config).expect("discover");
    let ids: Vec<_> = plugins.iter().map(|p| p.manifest.id.as_str()).collect();
    for expected in ["echo", "libro", "chirp", "graphicaudio"] {
        assert!(
            ids.contains(&expected),
            "expected plugin `{expected}` in {ids:?}"
        );
    }

    for plugin in &plugins {
        let client = PluginClient::spawn(
            &plugin.manifest.id,
            &plugin.command,
            &plugin.manifest.args,
            &plugin.root,
            serde_json::json!({}),
        )
        .await
        .unwrap_or_else(|e| panic!("spawn {}: {e}", plugin.manifest.id));
        let hs = client.handshake();
        assert_eq!(hs.id, plugin.manifest.id);
        assert_eq!(hs.api_version, bookclerk_plugin::PLUGIN_API_VERSION);
        match plugin.manifest.kind {
            PluginKind::Source => assert_eq!(hs.kind, "source"),
            PluginKind::Integration => assert_eq!(hs.kind, "integration"),
            other => panic!("unexpected kind {other:?}"),
        }
        let health: HealthDto = client
            .call(methods::HEALTH, serde_json::json!({}))
            .await
            .expect("health");
        assert!(
            health.ok,
            "{} health not ok: {:?}",
            plugin.manifest.id, health
        );
    }

    std::env::remove_var("BOOKCLERK_PLUGIN_DIRS");
}
