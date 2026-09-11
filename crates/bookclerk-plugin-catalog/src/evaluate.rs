//! Evaluate an installed plugin tree into [`PluginInstallIdentity`].

use std::path::Path;

use bookclerk_plugin_manifest::PluginManifest;

use crate::error::{CatalogError, Result};
use crate::identity::{
    is_platform_plugin_key, platform_artifact, ArtifactIdentity, PluginInstallIdentity, PluginKey,
    PluginProvenance, PLATFORM_PRODUCT,
};
use crate::payload::{manifest_sha256, payload_root_sha256};
use crate::receipt::InstallReceipt;

/// Evaluate identity + provenance for an install directory.
///
/// Receipt fields are **host-owned**. Plugin-authored `plugin.toml` cannot
/// claim platform provenance.
///
/// # Errors
///
/// Returns an error when the payload cannot be hashed, `plugin.toml` is
/// missing, or a receipt is malformed.
pub fn evaluate_install(root: &Path, manifest: &PluginManifest) -> Result<PluginInstallIdentity> {
    let manifest_sha = manifest_sha256(root)?;
    let payload_sha = payload_root_sha256(root)?;
    let version = manifest.version.clone().unwrap_or_else(|| "0.0.0".into());

    match InstallReceipt::load(root) {
        Ok(receipt) => {
            let plugin_key = receipt.plugin_key()?;
            if plugin_key.manifest_id() != manifest.id {
                return Err(CatalogError::message(format!(
                    "receipt plugin key `{}` does not match plugin.toml id `{}`",
                    plugin_key, manifest.id
                )));
            }
            let hashes_match = receipt.manifest_sha256.eq_ignore_ascii_case(&manifest_sha)
                && receipt
                    .payload_root_sha256
                    .eq_ignore_ascii_case(&payload_sha);
            let provenance = if !hashes_match {
                PluginProvenance::Modified
            } else if receipt.provenance == PluginProvenance::PlatformBundled
                && is_platform_plugin_key(&plugin_key)
                && platform_artifact(&plugin_key.package(), manifest.id.as_str()).is_some()
            {
                PluginProvenance::PlatformBundled
            } else if matches!(
                receipt.provenance,
                PluginProvenance::PlatformBundled | PluginProvenance::VerifiedInstalled
            ) {
                // Receipt claimed platform but the key is not a known platform
                // artifact: never honor plugin-controlled privilege.
                PluginProvenance::VerifiedInstalled
            } else {
                receipt.provenance
            };
            let artifact = ArtifactIdentity {
                plugin_key: plugin_key.clone(),
                version,
                manifest_sha256: manifest_sha,
                payload_root_sha256: payload_sha,
                archive_sha256: Some(receipt.archive_sha256.clone()).filter(|s| !s.is_empty()),
            };
            Ok(PluginInstallIdentity {
                plugin_key,
                artifact,
                provenance,
            })
        }
        Err(_) => {
            // No receipt: development / manually copied tree.
            let plugin_key = PluginKey::from_install_path(root, &manifest.id)?;
            let artifact = ArtifactIdentity {
                plugin_key: plugin_key.clone(),
                version,
                manifest_sha256: manifest_sha,
                payload_root_sha256: payload_sha,
                archive_sha256: None,
            };
            Ok(PluginInstallIdentity {
                plugin_key,
                artifact,
                provenance: PluginProvenance::LocalDevelopment,
            })
        }
    }
}

/// Build a host-owned platform-bundled receipt for a staged installer artifact.
///
/// Only [`crate::identity::PLATFORM_ARTIFACTS`] may be stamped this way.
///
/// # Errors
///
/// Returns an error when `package_name`/`manifest.id` are not a known
/// platform pair, or hashing fails.
pub fn stamp_platform_receipt(
    root: &Path,
    package_name: &str,
    manifest: &PluginManifest,
    version: &str,
) -> Result<InstallReceipt> {
    let Some(spec) = platform_artifact(package_name, &manifest.id) else {
        return Err(CatalogError::message(format!(
            "refusing platform receipt for `{package_name}` id `{}` (not a Bookclerk platform artifact)",
            manifest.id
        )));
    };
    let plugin_key = spec.plugin_key();
    let manifest_sha = manifest_sha256(root)?;
    let payload_sha = payload_root_sha256(root)?;
    let receipt = InstallReceipt::platform_bundled(
        plugin_key,
        version,
        manifest,
        manifest_sha,
        payload_sha,
        PLATFORM_PRODUCT,
        package_name,
    );
    receipt.store(root)?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::write_file;

    fn write_sqlite_tree(root: &Path) {
        write_file(
            &root.join("plugin.toml"),
            br#"
api_version = 3
id = "sqlite"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "deny"
"#,
        )
        .unwrap();
        write_file(&root.join("guest"), b"#!/bin/sh\n").unwrap();
    }

    fn parse_sqlite(root: &Path) -> PluginManifest {
        let text = std::fs::read_to_string(root.join("plugin.toml")).unwrap();
        PluginManifest::parse(&text).unwrap()
    }

    #[test]
    fn missing_receipt_is_local_development() {
        let dir = tempfile::tempdir().unwrap();
        write_sqlite_tree(dir.path());
        let manifest = parse_sqlite(dir.path());
        let identity = evaluate_install(dir.path(), &manifest).unwrap();
        assert_eq!(identity.provenance, PluginProvenance::LocalDevelopment);
        assert_eq!(identity.alias(), "sqlite");
        assert!(!is_platform_plugin_key(&identity.plugin_key));
    }

    #[test]
    fn stamped_platform_receipt_is_trusted_until_modified() {
        let dir = tempfile::tempdir().unwrap();
        write_sqlite_tree(dir.path());
        let manifest = parse_sqlite(dir.path());
        stamp_platform_receipt(
            dir.path(),
            "bookclerk-plugin-database-sqlite",
            &manifest,
            "0.1.0",
        )
        .unwrap();
        let identity = evaluate_install(dir.path(), &manifest).unwrap();
        assert_eq!(identity.provenance, PluginProvenance::PlatformBundled);
        assert!(identity.provenance.grants_platform_defaults());

        write_file(&dir.path().join("guest"), b"#!/bin/sh\n#pwn\n").unwrap();
        let identity = evaluate_install(dir.path(), &manifest).unwrap();
        assert_eq!(identity.provenance, PluginProvenance::Modified);
        assert!(!identity.provenance.grants_platform_defaults());
    }

    #[test]
    fn fake_sqlite_id_cannot_stamp_platform_receipt_with_wrong_package() {
        let dir = tempfile::tempdir().unwrap();
        write_sqlite_tree(dir.path());
        let manifest = parse_sqlite(dir.path());
        let err =
            stamp_platform_receipt(dir.path(), "evil-sqlite", &manifest, "1.0.0").unwrap_err();
        assert!(
            err.to_string().contains("not a Bookclerk platform"),
            "{err}"
        );
    }
}
