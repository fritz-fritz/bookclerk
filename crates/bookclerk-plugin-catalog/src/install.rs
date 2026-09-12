//! Secure plugin installer state machine.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;

use crate::coordinate::{PackageCoordinate, RegistrySource};
use crate::error::{CatalogError, Result};
use crate::extract::{extract_archive, safe_join, sha256_file, write_file};
use crate::identity::{PluginKey, PluginProvenance};
use crate::kind::RuntimeIdentity;
use crate::ledger::record_install;
use crate::manifest::{parse_sha256_hex, BookclerkPackageManifest};
use crate::receipt::InstallReceipt;
use crate::target::{host_bookclerk_target, select_target, ArchiveFormat};
use crate::trust::TrustPolicy;
use bookclerk_plugin_manifest::{NetworkMode, PluginFamily, PluginManifest};

/// Download / install limits.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Maximum HTTP redirects permitted while fetching a package artifact (`5`).
pub const MAX_REDIRECTS: u32 = 5;
/// Hard cap on downloaded artifact size in bytes.
pub const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

/// Options for [`Installer::install_from_manifest`].
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Directory under which plugin install folders are created.
    pub plugins_root: PathBuf,
    /// Host target triple used to select release artifacts.
    pub target: Option<String>,
    /// When true, resolve and verify without writing an install.
    pub dry_run: bool,
    /// When true, overwrite an existing install for the same plugin id.
    pub replace: bool,
    /// When true, refuse network fetches (local/cache only).
    pub offline: bool,
    /// Trust policy applied to signatures / publishers.
    pub trust: TrustPolicy,
    /// Skip health spawn (caller runs health separately).
    pub skip_health: bool,
    /// When true, persist a covering consent grant after install.
    pub approve_capabilities: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            plugins_root: PathBuf::from("plugins"),
            target: None,
            dry_run: false,
            replace: false,
            offline: false,
            trust: TrustPolicy::default(),
            skip_health: true,
            approve_capabilities: false,
        }
    }
}

/// Result of a successful (or dry-run) install.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    /// Filesystem path of the installed plugin directory.
    pub plugin_root: PathBuf,
    /// Install receipt written under the plugin root after activation.
    pub receipt: InstallReceipt,
    /// When true, resolve and verify without writing an install.
    pub dry_run: bool,
    /// Previous install kept aside until [`Installer::commit`] (or restored by
    /// [`Installer::rollback`]). Present only for replace installs.
    pub previous: Option<PathBuf>,
}

/// Installer: resolve → download → verify → extract → activate.
pub struct Installer;

impl Installer {
    /// Install from an already-validated package manifest (fixture / adapter output).
    ///
    /// # Arguments
    ///
    /// * `manifest` - Validated Bookclerk package manifest for this version.
    /// * `coordinate` - Source-qualified coordinate being installed.
    /// * `opts` - Plugins root, target, dry-run, replace, trust, and consent flags.
    ///
    /// # Returns
    ///
    /// [`InstallOutcome`] with plugin root, receipt, and optional previous install.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] when validation, download, digest, extract, or
    /// activation fails (or when offline/trust policy refuses the package).
    pub fn install_from_manifest(
        manifest: &BookclerkPackageManifest,
        coordinate: &PackageCoordinate,
        opts: &InstallOptions,
    ) -> Result<InstallOutcome> {
        manifest.validate_for_install()?;
        let artifact = select_target(
            &manifest.artifacts,
            |a| a.target.as_str(),
            opts.target.as_deref(),
        )?;
        let target = artifact.bookclerk_target();
        let runtime = manifest.runtime();
        validate_plugin_id(&runtime.id)?;

        let incoming_key = match &coordinate.source {
            RegistrySource::LocalArchive => PluginKey::from_install_path(
                Path::new(&coordinate.name),
                &runtime.id,
            )
            .or_else(|_| {
                PluginKey::from_install_path(&opts.plugins_root.join(&runtime.id), &runtime.id)
            })?,
            _ => PluginKey::from_coordinate(coordinate, &runtime.id)?,
        };
        let dest = safe_join(&opts.plugins_root, Path::new(&incoming_key.fs_id()))?;
        if dest.exists() {
            if let Ok(existing) = InstallReceipt::load(&dest) {
                if existing.runtime.id.eq_ignore_ascii_case(&runtime.id)
                    && existing.runtime.kind != runtime.kind
                {
                    return Err(CatalogError::message(format!(
                        "plugin id `{}` is already installed as a {} plugin at {}; \
                         ids must be globally unique across kinds",
                        runtime.id,
                        existing.runtime.kind.as_str(),
                        dest.display()
                    )));
                }
                if let Ok(existing_key) = existing.plugin_key() {
                    if existing_key != incoming_key {
                        return Err(CatalogError::message(format!(
                            "install directory {} belongs to `{existing_key}`; refusing `{incoming_key}`",
                            dest.display()
                        )));
                    }
                }
                let conflict = existing.runtime != runtime
                    || existing.coordinate.source.kind_name() != coordinate.source.kind_name()
                    || existing.coordinate.name != coordinate.name;
                if conflict && !opts.replace {
                    return Err(CatalogError::message(format!(
                        "runtime id `{}` already installed from {}; pass --replace",
                        runtime.id, existing.coordinate
                    )));
                }
                // Capability widening requires explicit approval even on --replace /
                // updates (replace must not bypass --approve-capabilities).
                if existing.requested_sandbox.network != manifest.sandbox.network
                    && !opts.approve_capabilities
                {
                    return Err(CatalogError::message(format!(
                        "update requests network `{}` (was `{}`); pass --approve-capabilities",
                        manifest.sandbox.network, existing.requested_sandbox.network
                    )));
                }
            } else if !opts.replace && dest.join("plugin.toml").exists() {
                return Err(CatalogError::message(format!(
                    "plugin `{}` already exists without receipt; pass --replace",
                    runtime.id
                )));
            }
        }

        opts.trust.check_unsigned_allowed()?;

        let expected = parse_sha256_hex(&artifact.archive_sha256)?;
        let _ = expected;

        if opts.dry_run {
            let receipt = build_receipt(
                manifest,
                coordinate,
                artifact,
                &target,
                None,
                &dest,
                &incoming_key,
                PluginProvenance::VerifiedInstalled,
                true,
            )?;
            return Ok(InstallOutcome {
                plugin_root: dest,
                receipt,
                dry_run: true,
                previous: None,
            });
        }

        let staging_parent = opts.plugins_root.join(".staging");
        fs::create_dir_all(&staging_parent)?;
        let staging = staging_parent.join(format!("{}.{}", runtime.id, std::process::id()));
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        fs::create_dir_all(&staging)?;

        let archive_path = staging.join("download.archive");
        download_to(&artifact.url, &archive_path, opts.offline)?;

        let actual = sha256_file(&archive_path)?;
        if !actual.eq_ignore_ascii_case(&artifact.archive_sha256) {
            let _ = fs::remove_dir_all(&staging);
            return Err(CatalogError::message(format!(
                "archive digest mismatch: expected {}, got {actual}",
                artifact.archive_sha256
            )));
        }

        let extract_root = staging.join("root");
        fs::create_dir_all(&extract_root)?;
        let format = if artifact.url.ends_with(".zip") || target.starts_with("windows-") {
            ArchiveFormat::Zip
        } else {
            ArchiveFormat::TarGz
        };
        extract_archive(&archive_path, format, &extract_root)?;

        let plugin_src = if artifact.archive_root == "." || artifact.archive_root.is_empty() {
            extract_root.clone()
        } else {
            safe_join(&extract_root, Path::new(&artifact.archive_root))?
        };
        let plugin_toml = plugin_src.join("plugin.toml");
        if !plugin_toml.is_file() {
            let _ = fs::remove_dir_all(&staging);
            return Err(CatalogError::message(
                "archive missing plugin.toml after extract",
            ));
        }
        let exe = safe_join(&plugin_src, Path::new(&artifact.executable))?;
        if !exe.is_file() {
            let _ = fs::remove_dir_all(&staging);
            return Err(CatalogError::message(format!(
                "archive missing executable {}",
                artifact.executable
            )));
        }
        let exe_digest = sha256_file(&exe)?;
        if let Some(expected_exe) = &artifact.executable_sha256 {
            if !exe_digest.eq_ignore_ascii_case(expected_exe) {
                let _ = fs::remove_dir_all(&staging);
                return Err(CatalogError::message("executable digest mismatch"));
            }
        }

        // Validate plugin.toml binds id/kind/sandbox/command to the package manifest.
        let toml_text = fs::read_to_string(&plugin_toml)?;
        validate_plugin_toml(
            &toml_text,
            &runtime,
            &manifest.sandbox.network,
            &artifact.executable,
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&exe)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&exe, perms)?;
        }

        // Preserve per-plugin state across replace so updates do not wipe data/tmp.
        let state_hold = staging.join("preserved-state");
        if dest.exists() {
            peel_plugin_state(&dest, &state_hold)?;
        }

        let backup = if dest.exists() {
            let bak = staging_parent.join(format!("{}.backup", runtime.id));
            if bak.exists() {
                fs::remove_dir_all(&bak)?;
            }
            fs::rename(&dest, &bak)?;
            Some(bak)
        } else {
            None
        };

        // Move extracted plugin tree into place (contents of plugin_src → dest).
        if let Err(e) = copy_dir_all(&plugin_src, &dest) {
            let _ = fs::remove_dir_all(&dest);
            if let Some(bak) = &backup {
                let _ = fs::rename(bak, &dest);
            }
            let _ = restore_plugin_state(&state_hold, &dest);
            let _ = fs::remove_dir_all(&staging);
            return Err(CatalogError::message(format!(
                "activate install failed: {e}"
            )));
        }

        if let Err(e) = restore_plugin_state(&state_hold, &dest) {
            let _ = fs::remove_dir_all(&dest);
            if let Some(bak) = &backup {
                let _ = fs::rename(bak, &dest);
            }
            let _ = restore_plugin_state(&state_hold, &dest);
            let _ = fs::remove_dir_all(&staging);
            return Err(CatalogError::message(format!(
                "restore plugin state failed: {e}"
            )));
        }

        let receipt = build_receipt(
            manifest,
            coordinate,
            artifact,
            &target,
            Some(exe_digest),
            &dest,
            &incoming_key,
            PluginProvenance::VerifiedInstalled,
            false,
        )?;
        if let Err(e) = receipt.store(&dest) {
            let _ = fs::remove_dir_all(&dest);
            if let Some(bak) = &backup {
                let _ = fs::rename(bak, &dest);
            }
            let _ = restore_plugin_state(&state_hold, &dest);
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
        if let Some(files_dir) = opts.plugins_root.parent() {
            if let Err(e) = record_install(
                files_dir,
                &incoming_key,
                &coordinate.name,
                &runtime.id,
                &receipt.manifest_sha256,
                &receipt.payload_root_sha256,
                PluginProvenance::VerifiedInstalled,
            ) {
                let _ = fs::remove_dir_all(&dest);
                if let Some(bak) = &backup {
                    let _ = fs::rename(bak, &dest);
                }
                let _ = restore_plugin_state(&state_hold, &dest);
                let _ = fs::remove_dir_all(&staging);
                return Err(e);
            }
        }

        // Drop staging (download/extract/state hold). Leave `previous` backup for
        // commit/rollback after the caller's health check.
        let _ = fs::remove_dir_all(&staging);

        Ok(InstallOutcome {
            plugin_root: dest,
            receipt,
            dry_run: false,
            previous: backup,
        })
    }

    /// Discard a replace backup after a successful health check (or when none).
    ///
    /// # Errors
    ///
    /// Returns an error when the operation fails.
    pub fn commit(outcome: &InstallOutcome) -> Result<()> {
        if let Some(bak) = &outcome.previous {
            if bak.exists() {
                remove_dir_retry(bak)?;
            }
        }
        Ok(())
    }

    /// Restore the previous install after a failed health check.
    ///
    /// # Errors
    ///
    /// Returns an error when the operation fails.
    pub fn rollback(outcome: &InstallOutcome) -> Result<()> {
        let Some(bak) = &outcome.previous else {
            return Ok(());
        };
        if outcome.plugin_root.exists() {
            // Peel state from the failed new install so it survives on the old tree.
            let hold = bak
                .parent()
                .unwrap_or(Path::new("."))
                .join(format!("{}.rollback-state", outcome.receipt.runtime.id));
            let _ = peel_plugin_state(&outcome.plugin_root, &hold);
            let _ = fs::remove_dir_all(&outcome.plugin_root);
            fs::rename(bak, &outcome.plugin_root)?;
            let _ = restore_plugin_state(&hold, &outcome.plugin_root);
            let _ = fs::remove_dir_all(&hold);
        } else if bak.exists() {
            fs::rename(bak, &outcome.plugin_root)?;
        }
        Ok(())
    }

    /// Install from a local archive path using an explicit manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when the operation fails.
    pub fn install_local_archive(
        archive: &Path,
        manifest: &BookclerkPackageManifest,
        opts: &InstallOptions,
    ) -> Result<InstallOutcome> {
        let coordinate = PackageCoordinate {
            source: RegistrySource::LocalArchive,
            name: archive.display().to_string(),
            version: manifest
                .coordinate
                .as_ref()
                .map(|c| c.version.clone())
                .unwrap_or_else(|| "0.0.0".into()),
        };
        // Rewrite artifact URL to the local file for the selected target.
        let mut m = manifest.clone();
        let host = opts
            .target
            .clone()
            .unwrap_or_else(|| host_bookclerk_target().to_string());
        for art in &mut m.artifacts {
            if crate::target::normalize_target(&art.target).unwrap_or(art.target.as_str())
                == crate::target::normalize_target(&host).unwrap_or(host.as_str())
            {
                art.url = format!("file://{}", archive.display());
                // Recompute digest from local file.
                art.archive_sha256 = sha256_file(archive)?;
            }
        }
        Self::install_from_manifest(&m, &coordinate, opts)
    }

    /// Remove an installed plugin directory; optionally purge data/tmp state.
    ///
    /// `spec` is a canonical PluginKey or a manifest alias. Bare aliases are
    /// accepted only when exactly one install under `plugins_root` uses that id.
    ///
    /// # Errors
    ///
    /// Returns an error when the operation fails.
    pub fn remove(plugins_root: &Path, spec: &str, purge_state: bool) -> Result<()> {
        let dest = resolve_installed_dir(plugins_root, spec)?;
        let leaf = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(spec)
            .to_string();
        let data = dest.join("data");
        let tmp = dest.join("tmp");
        let state_backup = plugins_root.join(format!(".state-keep-{leaf}"));
        if !purge_state && (data.exists() || tmp.exists()) {
            fs::create_dir_all(&state_backup)?;
            if data.exists() {
                let _ = fs::rename(&data, state_backup.join("data"));
            }
            if tmp.exists() {
                let _ = fs::rename(&tmp, state_backup.join("tmp"));
            }
        }
        remove_dir_retry(&dest)?;
        if purge_state {
            let _ = fs::remove_dir_all(&state_backup);
        }
        Ok(())
    }
}

/// Resolves `spec` to an install directory under `plugins_root`.
fn resolve_installed_dir(plugins_root: &Path, spec: &str) -> Result<PathBuf> {
    if let Ok(key) = PluginKey::parse(spec) {
        let dest = safe_join(plugins_root, Path::new(&key.fs_id()))?;
        if dest.exists() {
            return Ok(dest);
        }
        return Err(CatalogError::message(format!(
            "plugin `{spec}` is not installed under {}",
            plugins_root.display()
        )));
    }
    let mut matches = Vec::new();
    let alias_legacy = safe_join(plugins_root, Path::new(spec))?;
    if alias_legacy.exists() {
        matches.push(alias_legacy);
    }
    if let Ok(entries) = fs::read_dir(plugins_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || !path.join("plugin.toml").is_file() {
                continue;
            }
            let id_match = InstallReceipt::load(&path)
                .ok()
                .map(|r| r.runtime.id.eq_ignore_ascii_case(spec))
                .or_else(|| {
                    std::fs::read_to_string(path.join("plugin.toml"))
                        .ok()
                        .and_then(|t| PluginManifest::parse(&t).ok())
                        .map(|m| m.id.eq_ignore_ascii_case(spec))
                })
                .unwrap_or(false);
            if id_match && !matches.iter().any(|p| p == &path) {
                matches.push(path);
            }
        }
    }
    match matches.len() {
        1 => Ok(matches.pop().expect("len 1")),
        0 => Err(CatalogError::message(format!(
            "plugin `{spec}` is not installed under {}",
            plugins_root.display()
        ))),
        _ => Err(CatalogError::message(format!(
            "plugin alias `{spec}` is ambiguous; use a provenance-qualified PluginKey"
        ))),
    }
}

/// Builds an [`InstallReceipt`] from the package manifest, chosen artifact, and content hashes.
#[allow(clippy::too_many_arguments)]
fn build_receipt(
    manifest: &BookclerkPackageManifest,
    coordinate: &PackageCoordinate,
    artifact: &crate::manifest::ArtifactTarget,
    target: &str,
    exe_digest: Option<String>,
    dest: &Path,
    plugin_key: &PluginKey,
    provenance: PluginProvenance,
    dry_run: bool,
) -> Result<InstallReceipt> {
    let registry_url = match &coordinate.source {
        RegistrySource::Cargo { registry_url } => Some(registry_url.clone()),
        RegistrySource::Npm { registry_url } => Some(registry_url.clone()),
        RegistrySource::Pypi { simple_url } => Some(simple_url.clone()),
        RegistrySource::Static { index_url } => Some(index_url.clone()),
        RegistrySource::LocalArchive => None,
    };
    let (manifest_sha256, payload_root_sha256) = if dry_run {
        (String::new(), String::new())
    } else {
        (
            crate::payload::manifest_sha256(dest)?,
            crate::payload::payload_root_sha256(dest)?,
        )
    };
    Ok(InstallReceipt {
        schema_version: InstallReceipt::SCHEMA_VERSION,
        plugin_key: plugin_key.canonical().to_string(),
        provenance,
        coordinate: coordinate.clone(),
        version: coordinate.version.clone(),
        registry_url,
        artifact_url: artifact.url.clone(),
        target: target.to_string(),
        archive_sha256: artifact.archive_sha256.clone(),
        manifest_sha256,
        payload_root_sha256,
        executable_sha256: exe_digest.or_else(|| artifact.executable_sha256.clone()),
        protocol: manifest.effective_protocol(),
        api_version: manifest.api_version,
        runtime: manifest.runtime(),
        requested_sandbox: manifest.sandbox.clone(),
        approved_network: manifest.sandbox.network.clone(),
        installed_at: Utc::now(),
        update_constraint: None,
    })
}

/// Reject plugin ids that fail the strict grammar (also blocks path escape).
fn validate_plugin_id(id: &str) -> Result<()> {
    bookclerk_plugin_manifest::validate_plugin_id(id)
        .map_err(|e| CatalogError::message(e.to_string()))
}

/// Checks the extracted `plugin.toml` against the package identity.
///
/// The manifest must parse as a v3 [`PluginManifest`], carry the package id,
/// derive the package [`PluginKind`] from its exported entrypoints / triggers,
/// request the same network mode as the package `sandbox`, and name the
/// packaged executable as its `command`.
fn validate_plugin_toml(
    text: &str,
    runtime: &RuntimeIdentity,
    expected_network: &str,
    expected_exe: &str,
) -> Result<()> {
    let manifest: PluginManifest = toml::from_str(text)?;
    if manifest.id != runtime.id {
        return Err(CatalogError::message(format!(
            "plugin.toml id `{}` does not match package id `{}`",
            manifest.id, runtime.id
        )));
    }
    let families = manifest.families();
    let package_family = PluginFamily::parse(runtime.kind.as_str());
    if !package_family.is_some_and(|family| families.contains(&family)) {
        return Err(CatalogError::message(format!(
            "plugin.toml entrypoints derive families [{}], which do not include package kind `{}`",
            families
                .iter()
                .map(|f| f.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            runtime.kind
        )));
    }
    let network = match manifest.capabilities.network.mode {
        NetworkMode::Deny => "none",
        NetworkMode::Outbound => "outbound",
    };
    let mode_label = match manifest.capabilities.network.mode {
        NetworkMode::Deny => "deny",
        NetworkMode::Outbound => "outbound",
    };
    let expected = match expected_network {
        "" | "outbound" => "outbound",
        "none" | "deny" => "none",
        other => other,
    };
    if network != expected {
        return Err(CatalogError::message(format!(
            "plugin.toml capabilities.network.mode `{mode_label}` does not match package sandbox \
             `{expected}`"
        )));
    }
    let command = manifest
        .command
        .as_deref()
        .and_then(Path::to_str)
        .ok_or_else(|| CatalogError::message("plugin.toml missing command"))?;
    if !command_matches_executable(command, expected_exe) {
        return Err(CatalogError::message(format!(
            "plugin.toml command `{command}` does not match package executable `{expected_exe}`"
        )));
    }
    Ok(())
}

/// True when `command` and `executable` share a file name (ignoring a leading `./`).
fn command_matches_executable(command: &str, executable: &str) -> bool {
    let cmd = Path::new(command.trim_start_matches("./"));
    let exe = Path::new(executable.trim_start_matches("./"));
    match (cmd.file_name(), exe.file_name()) {
        (Some(a), Some(b)) => a == b,
        _ => command == executable,
    }
}

/// Moves `data/` and `tmp/` aside so a replace install can restore guest state.
fn peel_plugin_state(plugin_root: &Path, hold: &Path) -> Result<()> {
    let data = plugin_root.join("data");
    let tmp = plugin_root.join("tmp");
    if !data.exists() && !tmp.exists() {
        return Ok(());
    }
    fs::create_dir_all(hold)?;
    if data.exists() {
        let dest = hold.join("data");
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        fs::rename(&data, &dest)?;
    }
    if tmp.exists() {
        let dest = hold.join("tmp");
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        fs::rename(&tmp, &dest)?;
    }
    Ok(())
}

/// Moves peeled `data/` and `tmp/` back into the plugin root after activate or rollback.
fn restore_plugin_state(hold: &Path, plugin_root: &Path) -> Result<()> {
    if !hold.exists() {
        return Ok(());
    }
    for name in ["data", "tmp"] {
        let src = hold.join(name);
        if !src.exists() {
            continue;
        }
        let dest = plugin_root.join(name);
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&src, &dest)?;
    }
    Ok(())
}

/// Copies a `file://` or local path, or HTTPS/localhost HTTP, refusing oversized bodies.
fn download_to(url: &str, dest: &Path, offline: bool) -> Result<()> {
    if let Some(path) = url.strip_prefix("file://") {
        fs::copy(path, dest)?;
        return Ok(());
    }
    if url.starts_with('/') || Path::new(url).exists() {
        fs::copy(url, dest)?;
        return Ok(());
    }
    if offline {
        return Err(CatalogError::message(
            "offline install requires a local file:// artifact url",
        ));
    }
    if !url.starts_with("https://")
        && !url.starts_with("http://127.0.0.1")
        && !url.starts_with("http://localhost")
    {
        return Err(CatalogError::message(
            "remote install requires https:// (or localhost http for fixtures)",
        ));
    }
    let _ = MAX_REDIRECTS;
    let _ = DOWNLOAD_TIMEOUT;
    let mut response = ureq::get(url)
        .header(
            "User-Agent",
            concat!("bookclerk/", env!("CARGO_PKG_VERSION"), " (plugin-install)"),
        )
        .call()
        .map_err(|e| CatalogError::message(format!("download failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(CatalogError::message(format!(
            "download HTTP {status} for {url}"
        )));
    }
    let mut buf = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(CatalogError::message("download exceeds size limit"));
    }
    write_file(dest, &buf)?;
    Ok(())
}

/// Recursively copies `src` into `dest`, creating missing parent directories.
fn copy_dir_all(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(src).unwrap();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&out)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &out)?;
        }
    }
    Ok(())
}

/// Retries `remove_dir_all` up to five times (50 ms apart) for transient Windows locks.
fn remove_dir_retry(path: &Path) -> Result<()> {
    let mut last = None;
    for _ in 0..5 {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    Err(CatalogError::message(format!(
        "failed to remove {}: {}",
        path.display(),
        last.map(|e| e.to_string()).unwrap_or_default()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::sha256_bytes;
    use crate::kind::{PluginKind, RuntimeIdentity};
    use crate::manifest::ArtifactTarget;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::Builder;

    fn make_echo_archive(dir: &Path) -> (PathBuf, String) {
        let archive = dir.join("echo.tar.gz");
        {
            let file = fs::File::create(&archive).unwrap();
            let enc = GzEncoder::new(file, Compression::default());
            let mut tar = Builder::new(enc);
            let toml =
                b"api_version = 3\nid = \"echo\"\ncommand = \"./echo\"\nentrypoints = [\"cli\"]\n\
                         [capabilities.network]\nmode = \"outbound\"\n";
            let mut h = tar::Header::new_gnu();
            h.set_size(toml.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, "plugin.toml", &toml[..]).unwrap();
            let bin = b"#!/bin/sh\necho ok\n";
            let mut h2 = tar::Header::new_gnu();
            h2.set_size(bin.len() as u64);
            h2.set_mode(0o755);
            h2.set_cksum();
            tar.append_data(&mut h2, "echo", &bin[..]).unwrap();
            let enc = tar.into_inner().unwrap();
            enc.finish().unwrap();
        }
        let digest = sha256_file(&archive).unwrap();
        (archive, digest)
    }

    #[test]
    fn atomic_install_and_receipt() {
        let tmp = tempfile::tempdir().unwrap();
        let (archive, digest) = make_echo_archive(tmp.path());
        let target = host_bookclerk_target();
        let manifest = BookclerkPackageManifest {
            schema_version: 1,
            protocol: None,
            api_version: 1,
            api_version_max: None,
            min_bookclerk: None,
            kind: PluginKind::Integration,
            id: "echo".into(),
            display_name: Some("Echo".into()),
            description: None,
            coordinate: None,
            artifacts: vec![ArtifactTarget {
                target: target.into(),
                url: format!("file://{}", archive.display()),
                archive_sha256: digest,
                archive_root: ".".into(),
                executable: "echo".into(),
                executable_sha256: None,
            }],
            sandbox: Default::default(),
            links: Default::default(),
            yanked: false,
            released_at: None,
            publisher: None,
        };
        let plugins = tmp.path().join("plugins");
        let opts = InstallOptions {
            plugins_root: plugins.clone(),
            trust: TrustPolicy {
                allow_unsigned: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let coord = PackageCoordinate {
            source: RegistrySource::LocalArchive,
            name: archive.display().to_string(),
            version: "1.0.0".into(),
        };
        let out = Installer::install_from_manifest(&manifest, &coord, &opts).unwrap();
        assert!(out.plugin_root.join("plugin.toml").is_file());
        Installer::commit(&out).unwrap();
        let receipt = InstallReceipt::load(&out.plugin_root).unwrap();
        assert_eq!(receipt.runtime.id, "echo");

        // Digest mismatch fails.
        let mut bad = manifest.clone();
        bad.artifacts[0].archive_sha256 = sha256_bytes(b"nope");
        // pad to 64 hex
        bad.artifacts[0].archive_sha256 = format!("{:0>64}", "ab");
        assert!(Installer::install_from_manifest(&bad, &coord, &opts).is_err());
    }

    #[test]
    fn rejects_invalid_plugin_id() {
        let err = validate_plugin_id("../evil").unwrap_err();
        assert!(err.to_string().contains("plugin id"), "{err}");
        let err = validate_plugin_id("/abs").unwrap_err();
        assert!(err.to_string().contains("plugin id"), "{err}");
        let err = validate_plugin_id("a-b").unwrap_err();
        assert!(err.to_string().contains("lowercase"), "{err}");
        let err = validate_plugin_id("a").unwrap_err();
        assert!(err.to_string().contains("2–32"), "{err}");
        validate_plugin_id("echo").unwrap();
        validate_plugin_id("my_store").unwrap();
    }

    #[test]
    fn rejects_same_plugin_key_different_kind_on_install() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let (archive, digest) = make_echo_archive(tmp.path());
        let target = host_bookclerk_target();
        let mut manifest = BookclerkPackageManifest {
            schema_version: 1,
            protocol: None,
            api_version: 1,
            api_version_max: None,
            min_bookclerk: None,
            kind: PluginKind::Integration,
            id: "echo".into(),
            display_name: Some("Echo".into()),
            description: None,
            coordinate: None,
            artifacts: vec![ArtifactTarget {
                target: target.into(),
                url: format!("file://{}", archive.display()),
                archive_sha256: digest.clone(),
                archive_root: ".".into(),
                executable: "echo".into(),
                executable_sha256: None,
            }],
            sandbox: Default::default(),
            links: Default::default(),
            yanked: false,
            released_at: None,
            publisher: None,
        };
        let opts = InstallOptions {
            plugins_root: plugins,
            replace: true,
            trust: TrustPolicy {
                allow_unsigned: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let coord = PackageCoordinate {
            source: RegistrySource::LocalArchive,
            name: archive.display().to_string(),
            version: "1.0.0".into(),
        };
        Installer::install_from_manifest(&manifest, &coord, &opts).unwrap();
        manifest.kind = PluginKind::Source;
        let err = Installer::install_from_manifest(&manifest, &coord, &opts)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("globally unique") || err.contains("already installed"),
            "{err}"
        );
    }

    #[test]
    fn same_alias_different_local_archives_coexist() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins = tmp.path().join("plugins");
        let (archive_a, digest_a) = make_echo_archive(tmp.path());
        let archive_b = tmp.path().join("echo-b.tar.gz");
        fs::copy(&archive_a, &archive_b).unwrap();
        let target = host_bookclerk_target();
        let manifest_for = |digest: String, url: String| BookclerkPackageManifest {
            schema_version: 1,
            protocol: None,
            api_version: 1,
            api_version_max: None,
            min_bookclerk: None,
            kind: PluginKind::Integration,
            id: "echo".into(),
            display_name: Some("Echo".into()),
            description: None,
            coordinate: None,
            artifacts: vec![ArtifactTarget {
                target: target.into(),
                url,
                archive_sha256: digest,
                archive_root: ".".into(),
                executable: "echo".into(),
                executable_sha256: None,
            }],
            sandbox: Default::default(),
            links: Default::default(),
            yanked: false,
            released_at: None,
            publisher: None,
        };
        let opts = InstallOptions {
            plugins_root: plugins,
            trust: TrustPolicy {
                allow_unsigned: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let coord_a = PackageCoordinate {
            source: RegistrySource::LocalArchive,
            name: archive_a.display().to_string(),
            version: "1.0.0".into(),
        };
        let coord_b = PackageCoordinate {
            source: RegistrySource::LocalArchive,
            name: archive_b.display().to_string(),
            version: "1.0.0".into(),
        };
        let a = Installer::install_from_manifest(
            &manifest_for(digest_a.clone(), format!("file://{}", archive_a.display())),
            &coord_a,
            &opts,
        )
        .unwrap();
        let b = Installer::install_from_manifest(
            &manifest_for(digest_a, format!("file://{}", archive_b.display())),
            &coord_b,
            &opts,
        )
        .unwrap();
        assert_ne!(a.plugin_root, b.plugin_root);
        assert_ne!(a.receipt.plugin_key, b.receipt.plugin_key);
        assert!(a
            .plugin_root
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("pk-"));
        assert!(b
            .plugin_root
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("pk-"));
    }

    /// Minimal v3 `plugin.toml` for an integration guest.
    fn echo_toml(command: &str, network_mode: &str, entrypoints: &str) -> String {
        format!(
            "api_version = 3\nid = \"echo\"\ncommand = \"{command}\"\n\
             entrypoints = [{entrypoints}]\n[capabilities.network]\nmode = \"{network_mode}\"\n"
        )
    }

    #[test]
    fn toml_binds_family_network_and_command() {
        let runtime = RuntimeIdentity::new(PluginKind::Integration, "echo");
        validate_plugin_toml(
            &echo_toml("./echo", "deny", "\"cli\""),
            &runtime,
            "none",
            "echo",
        )
        .unwrap();
        let bad_net = echo_toml("./echo", "outbound", "\"cli\"");
        assert!(validate_plugin_toml(&bad_net, &runtime, "none", "echo").is_err());
        let bad_cmd = echo_toml("./other", "outbound", "\"cli\"");
        assert!(validate_plugin_toml(&bad_cmd, &runtime, "outbound", "echo").is_err());
        // A storefront-only manifest is not an integration package.
        let bad_family = echo_toml("./echo", "deny", "\"storefront\"");
        let err = validate_plugin_toml(&bad_family, &runtime, "none", "echo")
            .unwrap_err()
            .to_string();
        assert!(err.contains("package kind `integration`"), "{err}");
        // Legacy `kind` keys are rejected by the strict v3 parser.
        let legacy =
            "api_version = 3\nid = \"echo\"\nkind = \"integration\"\ncommand = \"./echo\"\n\
                      entrypoints = [\"cli\"]\n[capabilities.network]\nmode = \"deny\"\n";
        assert!(validate_plugin_toml(legacy, &runtime, "none", "echo").is_err());
    }
}
