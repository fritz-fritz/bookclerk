//! Scan plugin directories for `plugin.toml` manifests.

use std::path::{Path, PathBuf};

use libation_config::Config;

use crate::manifest::PluginManifest;
use crate::{PluginError, Result};

/// A discovered plugin ready to spawn.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub manifest: PluginManifest,
    /// Directory containing `plugin.toml`.
    pub root: PathBuf,
    /// Absolute path to the plugin executable.
    pub command: PathBuf,
}

/// Resolve search roots: `LIBATION_PLUGIN_DIRS` then `$FILES_DIR/plugins`.
#[must_use]
pub fn plugin_search_dirs(config: &Config) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(raw) = std::env::var("LIBATION_PLUGIN_DIRS") {
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
pub fn discover_plugins(config: &Config) -> Result<Vec<DiscoveredPlugin>> {
    let mut out = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for dir in plugin_search_dirs(config) {
        if !dir.is_dir() {
            continue;
        }
        discover_in_dir(&dir, &mut out, &mut seen_ids)?;
    }
    out.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    Ok(out)
}

fn discover_in_dir(
    dir: &Path,
    out: &mut Vec<DiscoveredPlugin>,
    seen_ids: &mut std::collections::HashSet<String>,
) -> Result<()> {
    let root_manifest = dir.join("plugin.toml");
    if root_manifest.is_file() {
        push_manifest(&root_manifest, dir, out, seen_ids)?;
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
            push_manifest(&manifest_path, &path, out, seen_ids)?;
        }
    }
    Ok(())
}

fn push_manifest(
    manifest_path: &Path,
    root: &Path,
    out: &mut Vec<DiscoveredPlugin>,
    seen_ids: &mut std::collections::HashSet<String>,
) -> Result<()> {
    let text = std::fs::read_to_string(manifest_path)?;
    let manifest = PluginManifest::parse(&text)?;
    if manifest.api_version > crate::PLUGIN_API_VERSION {
        tracing::warn!(
            id = %manifest.id,
            plugin_api = manifest.api_version,
            host_api = crate::PLUGIN_API_VERSION,
            "plugin api_version newer than host; skipping"
        );
        return Ok(());
    }
    if !seen_ids.insert(manifest.id.clone()) {
        tracing::warn!(
            id = %manifest.id,
            path = %manifest_path.display(),
            "duplicate plugin id; keeping first discovery"
        );
        return Ok(());
    }
    let command = resolve_command(root, &manifest.command)?;
    if !command.is_file() {
        return Err(PluginError::message(format!(
            "plugin `{}`: command not found at {}",
            manifest.id,
            command.display()
        )));
    }
    out.push(DiscoveredPlugin {
        manifest,
        root: root.to_path_buf(),
        command,
    });
    Ok(())
}

fn resolve_command(root: &Path, command: &Path) -> Result<PathBuf> {
    if command.is_absolute() {
        return Ok(command.to_path_buf());
    }
    let joined = root.join(command);
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn discovers_nested_plugin_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let plug = tmp.path().join("echo");
        fs::create_dir_all(&plug).unwrap();
        let bin = plug.join("echo-bin");
        fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let mut perms = fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin, perms).unwrap();
        fs::write(
            plug.join("plugin.toml"),
            r#"
api_version = 1
id = "echo"
kind = "integration"
command = "./echo-bin"
"#,
        )
        .unwrap();

        let plugins_root = tmp.path().join("plugins");
        fs::create_dir_all(&plugins_root).unwrap();
        let nested = plugins_root.join("echo");
        fs::create_dir_all(&nested).unwrap();
        fs::copy(plug.join("plugin.toml"), nested.join("plugin.toml")).unwrap();
        fs::copy(&bin, nested.join("echo-bin")).unwrap();
        let mut perms = fs::metadata(nested.join("echo-bin")).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(nested.join("echo-bin"), perms).unwrap();

        let cfg = Config {
            paths: Some(libation_config::Paths::from_files_dir(
                tmp.path().to_path_buf(),
            )),
            ..Config::default()
        };
        let found = discover_plugins(&cfg).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.id, "echo");
    }
}
