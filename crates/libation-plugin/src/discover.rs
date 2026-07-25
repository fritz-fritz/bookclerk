//! Discover external plugins declared in `config.toml`.
//!
//! A `[sources.<id>]` or `[integrations.<id>]` table is treated as an external
//! plugin when it sets `command` (path to an executable). Kind is inferred from
//! the section. Remaining keys (minus host-owned `command` / `args`) are passed
//! to the plugin on handshake.

use std::path::{Path, PathBuf};

use libation_config::Config;

use crate::manifest::PluginKind;
use crate::{PluginError, Result};

/// Host-owned keys on a plugin table (not forwarded as opaque knobs).
const HOST_KEYS: &[&str] = &["command", "args"];

/// A plugin declared in config and ready to spawn.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub id: String,
    pub kind: PluginKind,
    /// Absolute or PATH-resolvable executable.
    pub command: PathBuf,
    pub args: Vec<String>,
    /// Working directory for the child (`files_dir`).
    pub cwd: PathBuf,
    /// Opaque knobs for handshake (host keys stripped).
    pub config: toml::Table,
}

/// Discover external plugins from `[sources.*]` / `[integrations.*]` tables
/// that declare `command`.
pub fn discover_plugins(config: &Config) -> Result<Vec<DiscoveredPlugin>> {
    let files_dir = config.paths().files_dir.clone();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (id, value) in &config.sources.plugins {
        if let Some(plugin) =
            plugin_from_table(id, PluginKind::Source, value, &files_dir, &mut seen)?
        {
            out.push(plugin);
        }
    }
    for (id, value) in &config.integrations.plugins {
        if let Some(plugin) =
            plugin_from_table(id, PluginKind::Integration, value, &files_dir, &mut seen)?
        {
            out.push(plugin);
        }
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn plugin_from_table(
    id: &str,
    kind: PluginKind,
    value: &toml::Value,
    files_dir: &Path,
    seen: &mut std::collections::HashSet<String>,
) -> Result<Option<DiscoveredPlugin>> {
    let Some(table) = value.as_table() else {
        return Ok(None);
    };
    let Some(cmd_val) = table.get("command") else {
        return Ok(None);
    };
    let Some(cmd_str) = cmd_val.as_str() else {
        return Err(PluginError::message(format!(
            "[{}.{}]: `command` must be a string",
            kind.as_str(),
            id
        )));
    };
    if cmd_str.trim().is_empty() {
        return Err(PluginError::message(format!(
            "[{}.{}]: `command` must not be empty",
            kind.as_str(),
            id
        )));
    }

    let args = parse_args(table.get("args"), kind, id)?;
    let command = resolve_command(files_dir, cmd_str);
    if command_looks_like_path(cmd_str) && !command.is_file() {
        return Err(PluginError::message(format!(
            "[{}.{}]: command not found at {}",
            kind.as_str(),
            id,
            command.display()
        )));
    }

    let dedupe_key = format!("{}:{}", kind.as_str(), id);
    if !seen.insert(dedupe_key) {
        tracing::warn!(
            id,
            kind = kind.as_str(),
            "duplicate plugin declaration; keeping first"
        );
        return Ok(None);
    }

    Ok(Some(DiscoveredPlugin {
        id: id.to_string(),
        kind,
        command,
        args,
        cwd: files_dir.to_path_buf(),
        config: strip_host_keys(table),
    }))
}

fn parse_args(value: Option<&toml::Value>, kind: PluginKind, id: &str) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(arr) = value.as_array() else {
        return Err(PluginError::message(format!(
            "[{}.{}]: `args` must be an array of strings",
            kind.as_str(),
            id
        )));
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let Some(s) = item.as_str() else {
            return Err(PluginError::message(format!(
                "[{}.{}]: `args[{i}]` must be a string",
                kind.as_str(),
                id
            )));
        };
        out.push(s.to_string());
    }
    Ok(out)
}

fn strip_host_keys(table: &toml::Table) -> toml::Table {
    let mut out = toml::Table::new();
    for (k, v) in table {
        if HOST_KEYS.contains(&k.as_str()) {
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    out
}

fn command_looks_like_path(command: &str) -> bool {
    let p = Path::new(command);
    p.is_absolute() || command.contains('/') || command.contains('\\') || command.starts_with('.')
}

fn resolve_command(files_dir: &Path, command: &str) -> PathBuf {
    let p = Path::new(command);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    if command_looks_like_path(command) {
        let joined = files_dir.join(p);
        if joined.exists() {
            return joined;
        }
        return joined;
    }
    // Bare name: let the OS search `PATH` at spawn time.
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn cfg_with_files_dir(dir: &Path) -> Config {
        Config {
            paths: Some(libation_config::Paths::from_files_dir(dir.to_path_buf())),
            ..Config::default()
        }
    }

    #[test]
    fn discovers_integration_from_config_command() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("plugins/echo/echo-bin");
        fs::create_dir_all(bin.parent().unwrap()).unwrap();
        fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let mut perms = fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin, perms).unwrap();

        let mut cfg = cfg_with_files_dir(tmp.path());
        let table = cfg.integrations.plugin_table_mut("echo");
        table.insert("enabled".into(), toml::Value::Boolean(true));
        table.insert(
            "command".into(),
            toml::Value::String("plugins/echo/echo-bin".into()),
        );
        table.insert("greeting".into(), toml::Value::String("hello".into()));

        let found = discover_plugins(&cfg).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "echo");
        assert_eq!(found[0].kind, PluginKind::Integration);
        assert_eq!(found[0].command, bin);
        assert!(!found[0].config.contains_key("command"));
        assert_eq!(
            found[0].config.get("greeting").and_then(|v| v.as_str()),
            Some("hello")
        );
        assert_eq!(
            found[0].config.get("enabled").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn ignores_tables_without_command() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = cfg_with_files_dir(tmp.path());
        cfg.sources.set_enabled("audible", true);
        cfg.sources.set_string("audible", "bitrate", "high");
        assert!(discover_plugins(&cfg).unwrap().is_empty());
    }

    #[test]
    fn discovers_source_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("my-source");
        fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let mut perms = fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin, perms).unwrap();

        let mut cfg = cfg_with_files_dir(tmp.path());
        let table = cfg.sources.table_mut("acme");
        table.insert(
            "command".into(),
            toml::Value::String(bin.to_string_lossy().into_owned()),
        );

        let found = discover_plugins(&cfg).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, PluginKind::Source);
        assert_eq!(found[0].id, "acme");
    }
}
