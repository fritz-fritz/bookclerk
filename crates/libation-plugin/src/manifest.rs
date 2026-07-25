//! `plugin.toml` schema for discovered plugins.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Which Libation surface a plugin implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Source,
    Integration,
    Output,
}

impl PluginKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Integration => "integration",
            Self::Output => "output",
        }
    }
}

/// On-disk plugin descriptor (`plugin.toml`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// Protocol version this plugin speaks (`1` today).
    pub api_version: u32,
    /// Stable plugin id (must match `[sources.<id>]` / `[integrations.<id>]`).
    pub id: String,
    /// Human-facing name.
    #[serde(default)]
    pub name: Option<String>,
    /// Plugin kind.
    pub kind: PluginKind,
    /// Executable to spawn (absolute, or relative to the manifest directory).
    pub command: PathBuf,
    /// Extra argv after `command`.
    #[serde(default)]
    pub args: Vec<String>,
}

impl PluginManifest {
    /// Parse manifest TOML text.
    pub fn parse(text: &str) -> crate::Result<Self> {
        let m: Self = toml::from_str(text)?;
        if m.id.trim().is_empty() {
            return Err(crate::PluginError::message("plugin.toml: `id` is required"));
        }
        if m.api_version == 0 {
            return Err(crate::PluginError::message(
                "plugin.toml: `api_version` must be >= 1",
            ));
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_integration() {
        let m = PluginManifest::parse(
            r#"
api_version = 1
id = "echo"
kind = "integration"
command = "./echo-integration"
"#,
        )
        .unwrap();
        assert_eq!(m.id, "echo");
        assert_eq!(m.kind, PluginKind::Integration);
        assert!(m.args.is_empty());
    }
}
