//! Installed-plugin receipt / lock record.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::coordinate::PackageCoordinate;
use crate::error::{CatalogError, Result};
use crate::kind::RuntimeIdentity;
use crate::manifest::SandboxRequest;

/// Filename for the receipt beside `plugin.toml`.
pub const RECEIPT_FILE: &str = "receipt.json";
pub const RECEIPT_BACKUP: &str = "receipt.json.bak";

/// Record written after a successful install / update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallReceipt {
    /// Schema version.
    pub schema_version: u32,
    /// Coordinate.
    pub coordinate: PackageCoordinate,
    /// Version.
    pub version: String,
    /// Registry URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_url: Option<String>,
    /// Artifact URL.
    pub artifact_url: String,
    /// Target.
    pub target: String,
    /// Archive sha256.
    pub archive_sha256: String,
    /// Executable sha256.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_sha256: Option<String>,
    /// Protocol.
    pub protocol: String,
    /// API version.
    pub api_version: u32,
    /// Runtime.
    pub runtime: RuntimeIdentity,
    /// Requested sandbox.
    pub requested_sandbox: SandboxRequest,
    /// Host-approved network mode (may be stricter than requested).
    pub approved_network: String,
    /// Installed at.
    pub installed_at: DateTime<Utc>,
    /// Update constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_constraint: Option<String>,
    /// Publisher key Identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_key_id: Option<String>,
    /// Allow unsigned.
    #[serde(default)]
    pub allow_unsigned: bool,
}

impl InstallReceipt {
    /// Schema version.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Path to receipt inside an installed plugin directory.
    #[must_use]
    pub fn path_in(plugin_root: &Path) -> PathBuf {
        plugin_root.join(RECEIPT_FILE)
    }

    /// Load receipt from a plugin install directory.
    pub fn load(plugin_root: &Path) -> Result<Self> {
        let path = Self::path_in(plugin_root);
        let text = fs::read_to_string(&path)
            .map_err(|e| CatalogError::message(format!("read {}: {e}", path.display())))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Atomically write receipt (temp + rename).
    pub fn store(&self, plugin_root: &Path) -> Result<()> {
        fs::create_dir_all(plugin_root)?;
        let final_path = Self::path_in(plugin_root);
        let tmp = plugin_root.join(format!("{RECEIPT_FILE}.tmp"));
        let text = serde_json::to_string_pretty(self)?;
        fs::write(&tmp, text)?;
        if final_path.exists() {
            let _ = fs::copy(&final_path, plugin_root.join(RECEIPT_BACKUP));
        }
        fs::rename(&tmp, &final_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinate::RegistrySource;
    use crate::kind::PluginKind;
    use crate::manifest::PROTOCOL_WORKERS_RPC;

    #[test]
    fn receipt_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let receipt = InstallReceipt {
            schema_version: InstallReceipt::SCHEMA_VERSION,
            coordinate: PackageCoordinate {
                source: RegistrySource::LocalArchive,
                name: "/tmp/x.tar.gz".into(),
                version: "1.0.0".into(),
            },
            version: "1.0.0".into(),
            registry_url: None,
            artifact_url: "file:///tmp/x.tar.gz".into(),
            target: "linux-x64-gnu".into(),
            archive_sha256: "aa".repeat(32),
            executable_sha256: None,
            protocol: PROTOCOL_WORKERS_RPC.into(),
            api_version: 1,
            runtime: RuntimeIdentity::new(PluginKind::Integration, "echo"),
            requested_sandbox: SandboxRequest {
                network: "none".into(),
            },
            approved_network: "none".into(),
            installed_at: Utc::now(),
            update_constraint: None,
            publisher_key_id: None,
            allow_unsigned: true,
        };
        receipt.store(dir.path()).unwrap();
        let loaded = InstallReceipt::load(dir.path()).unwrap();
        assert_eq!(loaded.runtime.id, "echo");
        assert_eq!(loaded.version, "1.0.0");
    }
}
