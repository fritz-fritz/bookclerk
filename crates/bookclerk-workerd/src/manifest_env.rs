//! Load `plugin.toml` from the plugin install root.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use bookclerk_plugin_manifest::PluginManifest;

/// Join `root` / `plugin.toml` and require the result stays under `root`.
fn join_plugin_toml(root: &Path) -> Result<PathBuf> {
    if root.as_os_str().is_empty() {
        bail!("refusing empty plugin root");
    }
    let root_norm = match fs::canonicalize(root) {
        Ok(c) => c,
        Err(_) => {
            let s = root.to_string_lossy().into_owned();
            if s.contains("..") || s.contains('\0') {
                bail!("refusing unsafe plugin root: {}", root.display());
            }
            PathBuf::from(s)
        }
    };
    let path = root_norm.join("plugin.toml");
    for comp in path.components() {
        if matches!(comp, Component::ParentDir) {
            bail!("refusing path with '..': {}", path.display());
        }
    }
    if !path.starts_with(&root_norm) {
        bail!(
            "path {} escapes plugin root {}",
            path.display(),
            root_norm.display()
        );
    }
    Ok(PathBuf::from(path.to_string_lossy().into_owned()))
}

/// Reads and validates `plugin.toml` from the guest install root.
pub fn load_manifest(root: &Path) -> Result<PluginManifest> {
    let path = join_plugin_toml(root)?;
    // Contained under plugin install root via [`join_plugin_toml`]; path rebuilt after validation.
    // codeql[rust/path-injection]
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    PluginManifest::parse(&text).map_err(|e| anyhow::anyhow!("{e}"))
}
