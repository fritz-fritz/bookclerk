//! Scan plugin directories for `plugin.toml` manifests.
//!
//! Discovery is install-time only. User settings come from the matching
//! `[sources.<id>]` / `[integrations.<id>]` table in `config.toml` (alias)
//! and are attached when the plugin is spawned. Durable identity is
//! [`PluginKey`], not the manifest alias.

use std::path::{Path, PathBuf};

use bookclerk_config::{Config, DatabasePluginKind};
use bookclerk_library::BOOKCLERK_SCHEMA_NAMESPACE;
use bookclerk_plugin_abi::PRODUCT_API_VERSION;
use bookclerk_plugin_catalog::{evaluate_install, PluginInstallIdentity, PluginKey};

use crate::manifest::{PluginFamily, PluginManifest};
use crate::{PluginError, Result};

/// A discovered plugin ready to spawn.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    /// Parsed `plugin.toml` for this install directory.
    pub manifest: PluginManifest,
    /// Directory containing `plugin.toml` (cwd + relative `command` base).
    pub root: PathBuf,
    /// Absolute path to the plugin executable.
    pub command: PathBuf,
    /// Host-evaluated provenance-qualified identity.
    pub identity: PluginInstallIdentity,
}

impl DiscoveredPlugin {
    /// Builds a discovered plugin, evaluating content hashes and provenance.
    ///
    /// When the install tree cannot be hashed (tests that only write a
    /// guest binary), falls back to path provenance without platform trust.
    #[must_use]
    pub fn new(manifest: PluginManifest, root: PathBuf, command: PathBuf) -> Self {
        let identity =
            evaluate_install(&root, &manifest).unwrap_or_else(|_| local_identity(&root, &manifest));
        Self {
            manifest,
            root,
            command,
            identity,
        }
    }

    /// Display / CLI alias (`plugin.toml` id).
    #[must_use]
    pub fn alias(&self) -> &str {
        self.identity.alias()
    }

    /// Provenance-qualified key.
    #[must_use]
    pub fn plugin_key(&self) -> &PluginKey {
        &self.identity.plugin_key
    }
}

/// Path-only identity used when an install tree cannot be hashed (test fixtures).
fn local_identity(root: &Path, manifest: &PluginManifest) -> PluginInstallIdentity {
    let plugin_key = PluginKey::from_install_path(root, &manifest.id).unwrap_or_else(|_| {
        PluginKey::parse("path:file:///invalid-plugin-root#ab").expect("valid fallback id")
    });
    PluginInstallIdentity {
        artifact: bookclerk_plugin_catalog::ArtifactIdentity {
            plugin_key: plugin_key.clone(),
            version: manifest.version.clone().unwrap_or_else(|| "0.0.0".into()),
            manifest_sha256: String::new(),
            payload_root_sha256: String::new(),
            archive_sha256: None,
        },
        plugin_key,
        provenance: bookclerk_plugin_catalog::PluginProvenance::LocalDevelopment,
    }
}

/// Resolve search roots: `BOOKCLERK_PLUGIN_DIRS` then `$FILES_DIR/plugins`.
#[must_use]
pub fn plugin_search_dirs(config: &Config) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(raw) = std::env::var("BOOKCLERK_PLUGIN_DIRS") {
        for part in std::env::split_paths(&raw) {
            if !part.as_os_str().is_empty() {
                dirs.push(part);
            }
        }
    }
    dirs.push(config.paths().files_dir.join("plugins"));
    dirs
}

/// Discover plugins under the configured search directories.
///
/// Accepts either:
/// - `$dir/plugin.toml` (single plugin at root), or
/// - `$dir/<name>/plugin.toml` (one plugin per subdirectory).
///
/// Duplicate [`PluginKey`] values are a hard error. The same manifest alias
/// from different provenances is allowed; callers must use
/// [`resolve_plugin_ref`] which requires a qualified key when the alias is
/// ambiguous.
///
/// # Errors
///
/// Returns [`PluginError`] on duplicate keys, missing binaries, or I/O failures.
pub fn discover_plugins(config: &Config) -> Result<Vec<DiscoveredPlugin>> {
    let mut out = Vec::new();
    let mut seen: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    for dir in plugin_search_dirs(config) {
        if !dir.is_dir() {
            continue;
        }
        discover_in_dir(&dir, &mut out, &mut seen)?;
    }
    out.sort_by(|a, b| {
        a.manifest
            .id
            .cmp(&b.manifest.id)
            .then_with(|| a.plugin_key().canonical().cmp(b.plugin_key().canonical()))
    });
    Ok(out)
}

/// Config occupancy selector (`plugin = "…"`), or `alias` when the field is empty.
///
/// Occupancy is how host config names the guest that may use a singleton
/// slot (`[database].plugin`, `[output.s3].plugin`, `[sources.<id>].plugin`).
/// An empty field still means the display alias, which
/// [`resolve_plugin_slot`] accepts only when exactly one install uses it.
///
/// # Arguments
///
/// * `plugin_field` - Occupancy string from config (`plugin = "…"`).
/// * `alias` - Display alias used when `plugin_field` is empty.
#[must_use]
pub fn occupancy_spec<'a>(plugin_field: &'a str, alias: &'a str) -> &'a str {
    let spec = plugin_field.trim();
    if spec.is_empty() {
        alias
    } else {
        spec
    }
}

/// True when `plugin_key` / `alias` is the occupant named by `spec`.
///
/// A parseable PluginKey never falls through to an alias comparison, so a
/// miss cannot inherit another provenance's grant or session. Does not
/// detect alias twins — callers that load or privilege-check must require
/// a unique match ([`resolve_plugin_slot`], destination session lookup).
///
/// # Arguments
///
/// * `plugin_key` - Canonical [`PluginKey`] text.
/// * `alias` - Manifest display id.
/// * `spec` - Occupancy selector from [`occupancy_spec`] or a job/CLI id.
#[must_use]
pub fn identity_matches_occupancy(plugin_key: &str, alias: &str, spec: &str) -> bool {
    let spec = spec.trim();
    if spec.is_empty() {
        return false;
    }
    if let Ok(key) = PluginKey::parse(spec) {
        return plugin_key == key.canonical();
    }
    alias.eq_ignore_ascii_case(spec)
}

/// True when `plugin` is the occupant named by `spec` (PluginKey or alias).
///
/// Does not detect alias twins — loaders must call [`resolve_plugin_slot`].
///
/// # Arguments
///
/// * `plugin` - Candidate install.
/// * `spec` - Occupancy selector from [`occupancy_spec`].
#[must_use]
pub fn plugin_matches_occupancy(plugin: &DiscoveredPlugin, spec: &str) -> bool {
    identity_matches_occupancy(plugin.plugin_key().canonical(), plugin.alias(), spec)
}

/// Resolves `spec` among `plugins` without treating a vacant slot as an error.
///
/// `None` means nothing installed matches. An ambiguous alias is an error
/// so callers fail closed instead of spawning every twin or last-write-wins.
///
/// # Arguments
///
/// * `plugins` - Already-filtered family candidates.
/// * `spec` - Occupancy selector from [`occupancy_spec`].
///
/// # Errors
///
/// Returns [`PluginError`] when two installs share the alias (or PluginKey).
pub fn resolve_plugin_slot<'a>(
    plugins: &'a [DiscoveredPlugin],
    spec: &str,
) -> Result<Option<&'a DiscoveredPlugin>> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Ok(None);
    }
    if let Ok(key) = PluginKey::parse(spec) {
        let matches: Vec<_> = plugins.iter().filter(|p| p.plugin_key() == &key).collect();
        return match matches.as_slice() {
            [one] => Ok(Some(*one)),
            [] => Ok(None),
            _ => Err(PluginError::message(format!(
                "duplicate plugin key `{spec}`"
            ))),
        };
    }
    let lower = spec.to_ascii_lowercase();
    let matches: Vec<_> = plugins
        .iter()
        .filter(|p| p.alias().eq_ignore_ascii_case(&lower))
        .collect();
    match matches.as_slice() {
        [one] => Ok(Some(*one)),
        [] => Ok(None),
        many => {
            let keys: Vec<_> = many.iter().map(|p| p.plugin_key().canonical()).collect();
            Err(PluginError::message(format!(
                "plugin alias `{spec}` is ambiguous; use a provenance-qualified PluginKey. candidates: {}",
                keys.join(", ")
            )))
        }
    }
}

/// Resolves `spec` to a discovered plugin.
///
/// `spec` may be a canonical [`PluginKey`] or a manifest alias. Bare aliases
/// resolve only when exactly one discovered plugin uses that id.
///
/// # Errors
///
/// Returns an error when `spec` matches nothing or more than one plugin.
pub fn resolve_plugin_ref<'a>(
    plugins: &'a [DiscoveredPlugin],
    spec: &str,
) -> Result<&'a DiscoveredPlugin> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(PluginError::message("plugin reference must not be empty"));
    }
    match resolve_plugin_slot(plugins, spec)? {
        Some(plugin) => Ok(plugin),
        None if PluginKey::parse(spec).is_ok() => Err(PluginError::message(format!(
            "no plugin installed with key `{spec}`"
        ))),
        None => Err(PluginError::message(format!(
            "plugin `{spec}` is not installed"
        ))),
    }
}

/// True when occupancy `spec` names the display alias (or kind token) `alias`.
///
/// A PluginKey occupant matches its manifest id and first-party kind tokens
/// (`pg` ↔ `postgres`). A bare alias does **not** match a PluginKey string
/// passed as `alias` — pinning a key from alias occupancy is a different
/// occupant selector and must re-check consent.
///
/// # Arguments
///
/// * `spec` - Occupancy selector from config (`plugin = "…"`).
/// * `alias` - Display alias or first-party kind token being compared.
#[must_use]
pub fn occupancy_matches_alias(spec: &str, alias: &str) -> bool {
    let spec = spec.trim();
    let alias = alias.trim();
    if spec.is_empty() || alias.is_empty() {
        return false;
    }
    if spec.eq_ignore_ascii_case(alias) {
        return true;
    }
    if let Ok(key) = PluginKey::parse(spec) {
        if key.manifest_id().eq_ignore_ascii_case(alias) {
            return true;
        }
        return DatabasePluginKind::parse(key.manifest_id()).is_some()
            && DatabasePluginKind::parse(key.manifest_id()) == DatabasePluginKind::parse(alias)
            && PluginKey::parse(alias).is_err();
    }
    if PluginKey::parse(alias).is_ok() {
        return false;
    }
    let spec_kind = DatabasePluginKind::parse(spec);
    let alias_kind = DatabasePluginKind::parse(alias);
    spec_kind.is_some() && spec_kind == alias_kind
}

/// Writes this install's canonical PluginKey into each family occupancy field.
///
/// Call after flipping `enabled` (CLI / Settings). Occupancy is how loaders
/// name a singleton slot; stamping the key means a later alias twin cannot
/// steal the slot.
///
/// # Errors
///
/// Returns when the plugin exports `Storage` but is not `s3` or `local`
/// (those destinations are not mapped in `config.toml` yet).
pub fn stamp_occupancy_plugin_key(config: &mut Config, plugin: &DiscoveredPlugin) -> Result<()> {
    let canonical = plugin.plugin_key().canonical().to_string();
    for family in plugin.manifest.families() {
        match family {
            PluginFamily::Source => {
                config
                    .sources
                    .set_string(plugin.alias(), "plugin", canonical.clone());
            }
            PluginFamily::Integration => {
                config
                    .integrations
                    .plugin_table_mut(plugin.alias())
                    .insert("plugin".into(), toml::Value::String(canonical.clone()));
            }
            PluginFamily::Output if plugin.alias().eq_ignore_ascii_case("s3") => {
                config.output.s3.plugin = canonical.clone();
            }
            PluginFamily::Output if plugin.alias().eq_ignore_ascii_case("local") => {
                config.output.local.plugin = canonical.clone();
            }
            PluginFamily::Output => {
                return Err(PluginError::message(format!(
                    "output plugin `{}` enable/disable is not mapped to config.toml yet",
                    plugin.alias()
                )));
            }
            PluginFamily::Database => {
                config.database.plugin = canonical.clone();
            }
        }
    }
    Ok(())
}

/// Rewrites unique alias occupancy strings to canonical PluginKeys.
///
/// Ambiguous aliases and vacant slots are left unchanged so loaders still
/// fail closed. An empty `discovered` slice is a no-op so a settings
/// discovery timeout cannot wipe occupancy.
pub fn upgrade_unique_alias_occupancy(config: &mut Config, discovered: &[DiscoveredPlugin]) {
    if discovered.is_empty() {
        return;
    }
    upgrade_occupancy_field(&mut config.database.plugin, discovered);
    if !config.output.s3.plugin.trim().is_empty() || config.output.s3.enabled {
        let spec = occupancy_spec(&config.output.s3.plugin, "s3").to_string();
        if let Some(key) = unique_canonical_occupancy(&spec, discovered) {
            config.output.s3.plugin = key;
        }
    }
    if !config.output.local.plugin.trim().is_empty() || config.output.local.enabled {
        let spec = occupancy_spec(&config.output.local.plugin, "local").to_string();
        if let Some(key) = unique_canonical_occupancy(&spec, discovered) {
            config.output.local.plugin = key;
        }
    }
    let source_ids: Vec<String> = config.sources.plugins.keys().cloned().collect();
    for id in source_ids {
        let spec = occupancy_spec(config.sources.occupancy(&id), &id).to_string();
        if let Some(key) = unique_canonical_occupancy(&spec, discovered) {
            config.sources.set_string(&id, "plugin", key);
        }
    }
    let integration_ids: Vec<String> = config.integrations.plugins.keys().cloned().collect();
    for id in integration_ids {
        let spec = occupancy_spec(config.integrations.occupancy(&id), &id).to_string();
        if let Some(key) = unique_canonical_occupancy(&spec, discovered) {
            config
                .integrations
                .plugin_table_mut(&id)
                .insert("plugin".into(), toml::Value::String(key));
        }
    }
}

/// Canonical PluginKey when `spec` uniquely resolves and is not already that key.
fn unique_canonical_occupancy(spec: &str, discovered: &[DiscoveredPlugin]) -> Option<String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    match resolve_plugin_slot(discovered, spec) {
        Ok(Some(plugin)) => {
            let canonical = plugin.plugin_key().canonical();
            if spec == canonical {
                None
            } else {
                Some(canonical.to_string())
            }
        }
        _ => None,
    }
}

/// Replaces `field` with a unique PluginKey when `spec` is a unique alias.
fn upgrade_occupancy_field(field: &mut String, discovered: &[DiscoveredPlugin]) {
    if let Some(key) = unique_canonical_occupancy(field, discovered) {
        *field = key;
    }
}

/// Discovers `$dir/plugin.toml` or each `$dir/<name>/plugin.toml`; skips unreadable directories.
fn discover_in_dir(
    dir: &Path,
    out: &mut Vec<DiscoveredPlugin>,
    seen: &mut std::collections::HashMap<String, PathBuf>,
) -> Result<()> {
    let root_manifest = dir.join("plugin.toml");
    if root_manifest.is_file() {
        push_manifest(&root_manifest, dir, out, seen)?;
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!(path = %dir.display(), %err, "cannot read plugin directory");
            return Ok(());
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("plugin.toml");
        if manifest_path.is_file() {
            push_manifest(&manifest_path, &path, out, seen)?;
        }
    }
    Ok(())
}

/// Parses a manifest, rejects duplicate keys / missing binaries, and skips newer `api_version`.
fn push_manifest(
    manifest_path: &Path,
    root: &Path,
    out: &mut Vec<DiscoveredPlugin>,
    seen: &mut std::collections::HashMap<String, PathBuf>,
) -> Result<()> {
    let text = std::fs::read_to_string(manifest_path)?;
    let manifest = PluginManifest::parse(&text)?;
    if manifest.id == BOOKCLERK_SCHEMA_NAMESPACE {
        return Err(PluginError::message(format!(
            "plugin `{}`: id `{BOOKCLERK_SCHEMA_NAMESPACE}` is reserved for the host schema namespace",
            manifest.id
        )));
    }
    if manifest.api_version > PRODUCT_API_VERSION {
        tracing::warn!(
            id = %manifest.id,
            plugin_api = manifest.api_version,
            host_api = PRODUCT_API_VERSION,
            "plugin api_version newer than host; skipping"
        );
        return Ok(());
    }
    let command = resolve_spawn_command(root, &manifest)?;
    if !command.is_file() {
        return Err(PluginError::message(format!(
            "plugin `{}`: command not found at {}",
            manifest.id,
            command.display()
        )));
    }
    let plugin = DiscoveredPlugin::new(manifest, root.to_path_buf(), command);
    let key = plugin.plugin_key().canonical().to_string();
    if let Some(first_path) = seen.get(&key) {
        return Err(PluginError::message(format!(
            "duplicate plugin key `{}` (alias `{}`): already discovered at {} and also at {}",
            key,
            plugin.alias(),
            first_path.display(),
            manifest_path.display()
        )));
    }
    seen.insert(key, manifest_path.to_path_buf());
    out.push(plugin);
    Ok(())
}

/// Resolves the native guest binary or the host `bookclerk-workerd` helper.
fn resolve_spawn_command(root: &Path, manifest: &PluginManifest) -> Result<PathBuf> {
    use crate::manifest::PluginRuntimeKind;
    match manifest.runtime {
        PluginRuntimeKind::Native => {
            let command = manifest.command.as_ref().ok_or_else(|| {
                PluginError::message(format!(
                    "plugin `{}`: native runtime missing command",
                    manifest.id
                ))
            })?;
            resolve_command(root, command)
        }
        // Discovery only records where the launcher is; the pinned `workerd`
        // beside it is checked when the spawn plan resolves.
        PluginRuntimeKind::Workerd => crate::spawn_plan::locate_launcher(),
    }
}

/// Treats relative `command` as rooted at the plugin install directory.
fn resolve_command(root: &Path, command: &Path) -> Result<PathBuf> {
    if command.is_absolute() {
        return Ok(command.to_path_buf());
    }
    Ok(root.join(command))
}

/// Opaque knobs from main `config.toml` for this plugin id (by primary family).
#[must_use]
pub fn settings_table(config: &Config, plugin: &DiscoveredPlugin) -> toml::Table {
    settings_table_for(config, plugin, plugin.manifest.primary_family())
}

/// Resolves the `config.toml` table for `plugin` under one specific handler
/// `family`.
///
/// A plugin exporting several entrypoints (for example `storefront` + an
/// event consumer) owns one settings table per family (`[sources.<id>]` and
/// `[integrations.<id>]`); callers rendering per-family settings groups pick
/// the family explicitly instead of relying on [`PluginManifest::primary_family`].
///
/// [`PluginManifest::primary_family`]: crate::PluginManifest::primary_family
pub fn settings_table_for(
    config: &Config,
    plugin: &DiscoveredPlugin,
    family: crate::PluginFamily,
) -> toml::Table {
    match family {
        crate::PluginFamily::Source => config
            .sources
            .table(&plugin.manifest.id)
            .cloned()
            .unwrap_or_default(),
        crate::PluginFamily::Integration => {
            let mut table = config
                .integrations
                .plugin_table(&plugin.manifest.id)
                .cloned()
                .unwrap_or_default();
            inject_abs_api_key_from_env(&plugin.manifest.id, &mut table);
            table
        }
        crate::PluginFamily::Output if plugin.manifest.id == "s3" => {
            output_s3_settings_table(&config.output.s3)
        }
        crate::PluginFamily::Output if plugin.manifest.id == "local" => {
            output_local_settings_table(&config.output.local)
        }
        crate::PluginFamily::Database => database_settings_table(config, plugin),
        crate::PluginFamily::Output => toml::Table::new(),
    }
}

/// Serializes `[database.<id>]` for the matching database plugin id.
///
/// First-party ids use their typed config sections; third-party adapters get
/// the opaque `[database.<id>]` table (delivered as `DatabaseAdapterConfig`).
fn database_settings_table(config: &Config, plugin: &DiscoveredPlugin) -> toml::Table {
    let id = plugin.manifest.id.to_ascii_lowercase();
    let value = match id.as_str() {
        "sqlite" => toml::Value::try_from(&config.database.sqlite),
        "d1" => toml::Value::try_from(&config.database.d1),
        "postgres" => toml::Value::try_from(&config.database.postgres),
        _ => {
            return config
                .database
                .plugin_table(&plugin.manifest.id)
                .or_else(|| config.database.plugin_table(&id))
                .cloned()
                .unwrap_or_default();
        }
    };
    match value {
        Ok(toml::Value::Table(table)) => table,
        _ => toml::Table::new(),
    }
}

/// Serializes `[output.s3]` into the spawn settings table.
fn output_s3_settings_table(cfg: &bookclerk_config::OutputS3Config) -> toml::Table {
    match toml::Value::try_from(cfg) {
        Ok(toml::Value::Table(table)) => table,
        _ => toml::Table::new(),
    }
}

/// Serializes `[output.local]` into the spawn settings table.
fn output_local_settings_table(cfg: &bookclerk_config::OutputLocalConfig) -> toml::Table {
    match toml::Value::try_from(cfg) {
        Ok(toml::Value::Table(table)) => table,
        _ => toml::Table::new(),
    }
}

/// When ABS config lacks `api_key`, inject `BOOKCLERK_ABS_API_KEY` into the
/// spawn config table (plugin processes do not inherit Bookclerk env secrets).
fn inject_abs_api_key_from_env(plugin_id: &str, table: &mut toml::Table) {
    match plugin_id.trim().to_ascii_lowercase().as_str() {
        "audiobookshelf" | "abs" => {}
        _ => return,
    }
    let missing = table
        .get("api_key")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    if !missing {
        return;
    }
    if let Ok(v) = std::env::var("BOOKCLERK_ABS_API_KEY") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            table.insert("api_key".into(), toml::Value::String(trimmed.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// Sets the Unix execute bit so discovery tests can treat the stub as a command.
    fn chmod_exec(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
    }

    #[test]
    fn discovers_nested_plugin_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_root = tmp.path().join("plugins");
        let nested = plugins_root.join("echo");
        fs::create_dir_all(&nested).unwrap();
        let bin = nested.join("echo-bin");
        fs::write(&bin, b"#!/bin/sh\n").unwrap();
        chmod_exec(&bin);
        fs::write(
            nested.join("plugin.toml"),
            r#"
api_version = 3
id = "echo"
runtime = "native"
command = "./echo-bin"
entrypoints = ["cli"]

[capabilities.network]
mode = "deny"
"#,
        )
        .unwrap();

        let cfg = Config {
            paths: Some(bookclerk_config::Paths::from_files_dir(
                tmp.path().to_path_buf(),
            )),
            ..Config::default()
        };
        let found = discover_plugins(&cfg).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.id, "echo");
        assert_eq!(found[0].command, bin);

        // Settings come from main config, not the manifest.
        let mut cfg2 = cfg;
        cfg2.integrations
            .plugin_table_mut("echo")
            .insert("greeting".into(), toml::Value::String("hi".into()));
        let settings = settings_table(&cfg2, &found[0]);
        assert_eq!(
            settings.get("greeting").and_then(|v| v.as_str()),
            Some("hi")
        );
    }

    fn write_plugin(dir: &Path, id: &str, entrypoint: &str) {
        fs::create_dir_all(dir).unwrap();
        let bin = dir.join("bin");
        fs::write(&bin, b"#!/bin/sh\n").unwrap();
        chmod_exec(&bin);
        fs::write(
            dir.join("plugin.toml"),
            format!(
                r#"
api_version = 3
id = "{id}"
runtime = "native"
command = "./bin"
entrypoints = ["{entrypoint}"]

[capabilities.network]
mode = "deny"
"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn duplicate_alias_is_allowed_and_requires_qualified_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        write_plugin(&plugins.join("echo-a"), "echo", "cli");
        write_plugin(&plugins.join("echo-b"), "echo", "cli");

        let cfg = Config {
            paths: Some(bookclerk_config::Paths::from_files_dir(
                tmp.path().to_path_buf(),
            )),
            ..Config::default()
        };
        let found = discover_plugins(&cfg).unwrap();
        assert_eq!(found.len(), 2);
        assert_ne!(found[0].plugin_key(), found[1].plugin_key());
        let err = resolve_plugin_ref(&found, "echo").unwrap_err().to_string();
        assert!(err.contains("ambiguous"), "{err}");
        assert!(err.contains("PluginKey"), "{err}");
        let qualified = resolve_plugin_ref(&found, found[0].plugin_key().canonical()).unwrap();
        assert_eq!(qualified.plugin_key(), found[0].plugin_key());
        let err = resolve_plugin_slot(&found, "echo").unwrap_err().to_string();
        assert!(err.contains("ambiguous"), "{err}");
        let occupant = resolve_plugin_slot(&found, found[0].plugin_key().canonical()).unwrap();
        assert_eq!(occupant.unwrap().plugin_key(), found[0].plugin_key());
        assert!(resolve_plugin_slot(&found, "missing").unwrap().is_none());
        assert_eq!(occupancy_spec("", "s3"), "s3");
        assert_eq!(occupancy_spec("  ", "s3"), "s3");
        assert_eq!(
            occupancy_spec(found[0].plugin_key().canonical(), "s3"),
            found[0].plugin_key().canonical()
        );
        assert!(plugin_matches_occupancy(
            &found[0],
            found[0].plugin_key().canonical()
        ));
        assert!(!plugin_matches_occupancy(
            &found[1],
            found[0].plugin_key().canonical()
        ));
        assert!(plugin_matches_occupancy(&found[0], "echo"));
        assert!(plugin_matches_occupancy(&found[1], "echo"));
        assert!(identity_matches_occupancy(
            found[0].plugin_key().canonical(),
            found[0].alias(),
            found[0].plugin_key().canonical()
        ));
        assert!(!identity_matches_occupancy(
            found[1].plugin_key().canonical(),
            found[1].alias(),
            found[0].plugin_key().canonical()
        ));
        assert!(identity_matches_occupancy(
            found[0].plugin_key().canonical(),
            "echo",
            "ECHO"
        ));
        assert!(!identity_matches_occupancy(
            found[0].plugin_key().canonical(),
            "echo",
            ""
        ));
        assert!(occupancy_matches_alias(
            found[0].plugin_key().canonical(),
            "echo"
        ));
        assert!(!occupancy_matches_alias(
            "echo",
            found[0].plugin_key().canonical()
        ));
        let mut cfg_echo = cfg.clone();
        cfg_echo.integrations.set_enabled("echo", true);
        cfg_echo
            .integrations
            .plugin_table_mut("echo")
            .insert("plugin".into(), toml::Value::String("echo".into()));
        upgrade_unique_alias_occupancy(&mut cfg_echo, &found);
        assert_eq!(cfg_echo.integrations.occupancy("echo"), "echo");
    }

    #[test]
    fn occupancy_matches_alias_kind_tokens_and_keys() {
        assert!(occupancy_matches_alias("sqlite", "sqlite"));
        assert!(occupancy_matches_alias("pg", "postgres"));
        assert!(occupancy_matches_alias("postgres", "postgresql"));
        assert!(occupancy_matches_alias(
            "platform:bookclerk/sqlite#sqlite",
            "sqlite"
        ));
        assert!(occupancy_matches_alias(
            "platform:bookclerk/postgres#postgres",
            "pg"
        ));
        assert!(!occupancy_matches_alias(
            "sqlite",
            "platform:bookclerk/sqlite#sqlite"
        ));
        assert!(!occupancy_matches_alias(
            "platform:bookclerk/sqlite#sqlite",
            "postgres"
        ));
        assert!(!occupancy_matches_alias("", "sqlite"));
    }

    #[test]
    fn stamp_and_upgrade_unique_occupancy_to_plugin_key() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        write_plugin(&plugins.join("sqlite"), "sqlite", "databaseAdapter");
        write_plugin(&plugins.join("echo"), "echo", "cli");
        write_plugin(&plugins.join("s3"), "s3", "storage");

        let mut cfg = Config {
            paths: Some(bookclerk_config::Paths::from_files_dir(
                tmp.path().to_path_buf(),
            )),
            ..Config::default()
        };
        let found = discover_plugins(&cfg).unwrap();
        let sqlite = resolve_plugin_ref(&found, "sqlite").unwrap();
        stamp_occupancy_plugin_key(&mut cfg, sqlite).unwrap();
        assert_eq!(cfg.database.plugin, sqlite.plugin_key().canonical());

        let echo = resolve_plugin_ref(&found, "echo").unwrap();
        cfg.integrations.set_enabled("echo", true);
        stamp_occupancy_plugin_key(&mut cfg, echo).unwrap();
        assert_eq!(
            cfg.integrations.occupancy("echo"),
            echo.plugin_key().canonical()
        );

        let s3 = resolve_plugin_ref(&found, "s3").unwrap();
        cfg.output.s3.enabled = true;
        stamp_occupancy_plugin_key(&mut cfg, s3).unwrap();
        assert_eq!(cfg.output.s3.plugin, s3.plugin_key().canonical());

        cfg.database.plugin = "sqlite".into();
        cfg.integrations
            .plugin_table_mut("echo")
            .insert("plugin".into(), toml::Value::String("echo".into()));
        cfg.output.s3.plugin = "s3".into();
        upgrade_unique_alias_occupancy(&mut cfg, &found);
        assert_eq!(cfg.database.plugin, sqlite.plugin_key().canonical());
        assert_eq!(
            cfg.integrations.occupancy("echo"),
            echo.plugin_key().canonical()
        );
        assert_eq!(cfg.output.s3.plugin, s3.plugin_key().canonical());

        upgrade_unique_alias_occupancy(&mut cfg, &[]);
        assert_eq!(cfg.database.plugin, sqlite.plugin_key().canonical());
    }

    #[test]
    fn same_alias_different_family_is_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        write_plugin(&plugins.join("echo-src"), "echo", "storefront");
        write_plugin(&plugins.join("echo-int"), "echo", "cli");

        let cfg = Config {
            paths: Some(bookclerk_config::Paths::from_files_dir(
                tmp.path().to_path_buf(),
            )),
            ..Config::default()
        };
        let found = discover_plugins(&cfg).unwrap();
        assert_eq!(found.len(), 2);
        let err = resolve_plugin_ref(&found, "echo").unwrap_err().to_string();
        assert!(err.contains("ambiguous"), "{err}");
    }

    #[test]
    fn reserved_bookclerk_plugin_id_is_hard_error() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        write_plugin(&plugins.join("host"), "bookclerk", "cli");
        let cfg = Config {
            paths: Some(bookclerk_config::Paths::from_files_dir(
                tmp.path().to_path_buf(),
            )),
            ..Config::default()
        };
        let err = discover_plugins(&cfg).unwrap_err().to_string();
        assert!(err.contains("reserved"), "{err}");
        assert!(err.contains("bookclerk"), "{err}");
    }

    #[test]
    fn leftover_migration_plan_toml_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let nested = plugins.join("echo_sql");
        write_plugin(&nested, "echo_sql", "cli");
        fs::write(
            nested.join("plugin.toml"),
            r#"
api_version = 3
id = "echo_sql"
runtime = "native"
command = "./bin"
migration_plan = "migrations.toml"
entrypoints = ["cli"]

[capabilities.network]
mode = "deny"

[[databases]]
binding = "DB"
"#,
        )
        .unwrap();
        let cfg = Config {
            paths: Some(bookclerk_config::Paths::from_files_dir(
                tmp.path().to_path_buf(),
            )),
            ..Config::default()
        };
        let err = discover_plugins(&cfg).unwrap_err().to_string();
        assert!(
            err.contains("migration_plan") || err.to_lowercase().contains("unknown"),
            "{err}"
        );
    }
}
