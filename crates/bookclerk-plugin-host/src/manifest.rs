//! `plugin.toml` schema — greenfield Workers RPC / workerd install descriptor.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::protocol::CliSchema;

/// Which Bookclerk surface a plugin implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Source,
    Integration,
    Output,
    Database,
}

impl PluginKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Integration => "integration",
            Self::Output => "output",
            Self::Database => "database",
        }
    }
}

/// Guest runtime selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PluginRuntimeKind {
    /// OS binary speaking the Workers RPC ABI.
    #[default]
    Native,
    /// Bookclerk-shipped `bookclerk-workerd` + author modules.
    Workerd,
}

/// Network mode declared in `[capabilities.network]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    /// No outbound network.
    #[default]
    Deny,
    /// Outbound via host egress proxy; initial hosts must be in `domains`.
    Outbound,
}

/// `[capabilities.network]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NetworkCapabilities {
    pub mode: NetworkMode,
    /// Initial-request host allowlist (required when `mode = outbound`).
    #[serde(default)]
    pub domains: Vec<String>,
}

impl Default for NetworkCapabilities {
    fn default() -> Self {
        Self {
            mode: NetworkMode::Deny,
            domains: vec![],
        }
    }
}

/// `[capabilities.bindings]` — host stubs the guest expects.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct BindingCapabilities {
    pub config: bool,
    pub secrets: bool,
    pub plugin_kv: bool,
    pub work_fs: bool,
    pub oauth: bool,
}

/// `[capabilities.methods]` — declared RPC surface for discovery/consent.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct MethodCapabilities {
    #[serde(default)]
    pub list: Vec<String>,
}

/// Full `[capabilities]` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesManifest {
    pub network: NetworkCapabilities,
    #[serde(default)]
    pub bindings: BindingCapabilities,
    #[serde(default)]
    pub methods: MethodCapabilities,
}

/// `[runtime.workerd]` — WorkerCode-equivalent fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkerdRuntimeManifest {
    pub compatibility_date: String,
    #[serde(default)]
    pub compatibility_flags: Vec<String>,
    pub main_module: String,
    #[serde(default = "default_modules_dir")]
    pub modules_dir: String,
    #[serde(default = "default_entrypoint")]
    pub entrypoint: String,
    #[serde(default)]
    pub limits: WorkerdLimits,
}

fn default_modules_dir() -> String {
    "modules".into()
}

fn default_entrypoint() -> String {
    "default".into()
}

/// Optional workerd resource limits (host clamps).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerdLimits {
    pub cpu_ms: Option<u32>,
    pub subrequests: Option<u32>,
}

/// One module in `[[modules]]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModuleSpec {
    pub name: String,
    pub path: String,
    #[serde(default = "default_module_type")]
    #[serde(rename = "type")]
    pub module_type: String,
}

fn default_module_type() -> String {
    "js".into()
}

/// On-disk plugin descriptor (`plugin.toml`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// ABI version (`1`).
    pub api_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub kind: PluginKind,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub runtime: PluginRuntimeKind,
    /// Native executable (required when `runtime = native`).
    #[serde(default)]
    pub command: Option<PathBuf>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Workerd isolate config (required when `runtime = workerd`).
    #[serde(default)]
    pub workerd: Option<WorkerdRuntimeManifest>,
    #[serde(default)]
    pub modules: Vec<ModuleSpec>,
    pub capabilities: CapabilitiesManifest,
    #[serde(default)]
    pub cli: Option<CliSchema>,
}

impl PluginManifest {
    /// Domains used for consent UI and favicon hints.
    #[must_use]
    pub fn consent_domains(&self) -> &[String] {
        &self.capabilities.network.domains
    }

    /// Google favicon URL from the first consented domain (if any).
    #[must_use]
    pub fn google_favicon_url(&self) -> Option<String> {
        self.capabilities
            .network
            .domains
            .first()
            .map(|host| format!("https://www.google.com/s2/favicons?domain={host}&sz=128"))
    }

    /// Parse and validate manifest TOML.
    pub fn parse(text: &str) -> crate::Result<Self> {
        let m: Self = toml::from_str(text)?;
        if m.id.trim().is_empty() {
            return Err(crate::PluginError::message("plugin.toml: `id` is required"));
        }
        if m.api_version != 1 {
            return Err(crate::PluginError::message(
                "plugin.toml: `api_version` must be 1",
            ));
        }
        match m.runtime {
            PluginRuntimeKind::Native => {
                if m.command.as_ref().is_none_or(|c| c.as_os_str().is_empty()) {
                    return Err(crate::PluginError::message(
                        "plugin.toml: `command` is required when runtime = \"native\"",
                    ));
                }
            }
            PluginRuntimeKind::Workerd => {
                if m.workerd.is_none() {
                    return Err(crate::PluginError::message(
                        "plugin.toml: `[workerd]` / runtime.workerd is required when runtime = \"workerd\"",
                    ));
                }
                let w = m.workerd.as_ref().unwrap();
                if w.compatibility_date.trim().is_empty() {
                    return Err(crate::PluginError::message(
                        "plugin.toml: workerd.compatibility_date is required",
                    ));
                }
                if w.main_module.trim().is_empty() {
                    return Err(crate::PluginError::message(
                        "plugin.toml: workerd.main_module is required",
                    ));
                }
            }
        }
        if m.capabilities.network.mode == NetworkMode::Outbound
            && m.capabilities.network.domains.is_empty()
        {
            return Err(crate::PluginError::message(
                "plugin.toml: capabilities.network.domains is required when mode = \"outbound\"",
            ));
        }
        Ok(m)
    }

    /// Resolve the process to spawn (native command or bookclerk-workerd).
    #[must_use]
    pub fn spawn_command(&self) -> Option<&PathBuf> {
        match self.runtime {
            PluginRuntimeKind::Native => self.command.as_ref(),
            PluginRuntimeKind::Workerd => None, // host resolves bookclerk-workerd
        }
    }
}

/// Map manifest network + oauth binding to jail network policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JailNetworkNeed {
    None,
    Outbound,
    Listen,
}

impl PluginManifest {
    #[must_use]
    pub fn jail_network_need(&self) -> JailNetworkNeed {
        match self.capabilities.network.mode {
            NetworkMode::Deny => JailNetworkNeed::None,
            NetworkMode::Outbound if self.capabilities.bindings.oauth => JailNetworkNeed::Listen,
            NetworkMode::Outbound => JailNetworkNeed::Outbound,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_workerd_echo() {
        let m = PluginManifest::parse(
            r#"
api_version = 1
id = "echo"
kind = "integration"
version = "1.0.0"
runtime = "workerd"

[workerd]
compatibility_date = "2026-08-01"
main_module = "index.js"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
"#,
        )
        .unwrap();
        assert_eq!(m.runtime, PluginRuntimeKind::Workerd);
        assert_eq!(m.capabilities.network.mode, NetworkMode::Deny);
    }

    #[test]
    fn outbound_requires_domains() {
        let err = PluginManifest::parse(
            r#"
api_version = 1
id = "x"
kind = "source"
runtime = "native"
command = "./x"
[capabilities.network]
mode = "outbound"
"#,
        )
        .expect_err("domains required");
        assert!(err.to_string().contains("domains"), "{err}");
    }

    #[test]
    fn parse_native_with_domains() {
        let m = PluginManifest::parse(
            r#"
api_version = 1
id = "audible"
kind = "source"
runtime = "native"
command = "./bookclerk-plugin-source-audible"
[capabilities.network]
mode = "outbound"
domains = ["api.audible.com", "www.amazon.com"]
[capabilities.bindings]
oauth = true
secrets = true
"#,
        )
        .unwrap();
        assert_eq!(m.jail_network_need(), JailNetworkNeed::Listen);
        assert!(m.google_favicon_url().unwrap().contains("api.audible.com"));
    }
}
