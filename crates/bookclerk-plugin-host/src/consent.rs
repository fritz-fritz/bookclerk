//! Operator permission grants for plugin capabilities.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::manifest::{PluginManifest, PluginRuntimeKind, WorkerdLimits};
use crate::{PluginError, Result};

/// Filename under `$BOOKCLERK_FILES_DIR` for persisted grants.
pub const GRANTS_FILE: &str = "plugin-grants.json";

/// One approved grant snapshot for a plugin id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginGrant {
    /// Plugin id from `plugin.toml` (globally unique across kinds).
    pub plugin_id: String,
    /// Plugin kind string (`source`, `integration`, `output`, `database`).
    pub kind: String,
    /// Approved network mode: `deny` or `outbound`.
    pub network_mode: String,
    /// Approved initial outbound domain patterns (workerd allowlist).
    pub domains: BTreeSet<String>,
    /// Approved host binding names (`config`, `secrets`, `oauth`, …).
    pub bindings: BTreeSet<String>,
    /// Approved workerd compatibility flags from the consent snapshot.
    pub compatibility_flags: BTreeSet<String>,
    /// Optional workerd CPU budget override in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_ms: Option<u32>,
    /// Optional workerd outbound fetch / subrequest budget override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subrequests: Option<u32>,
    /// RFC 3339 time when the operator approved this grant.
    pub approved_at: String,
}

/// On-disk grant store.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginGrantStore {
    /// Persisted per-plugin grant snapshots.
    #[serde(default)]
    pub grants: Vec<PluginGrant>,
}

impl PluginGrantStore {
    /// Absolute path to `plugin-grants.json` under `files_dir`.
    pub fn path(files_dir: &Path) -> PathBuf {
        files_dir.join(GRANTS_FILE)
    }

    /// Loads grants from disk, or an empty store when the file is missing.
    ///
    /// # Arguments
    ///
    /// * `files_dir` - Bookclerk files directory that owns `plugin-grants.json`.
    ///
    /// # Returns
    ///
    /// Parsed [`PluginGrantStore`], defaulting when the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError`] when the file cannot be read or parsed.
    pub fn load(files_dir: &Path) -> Result<Self> {
        let path = Self::path(files_dir);
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Writes this grant store to `plugin-grants.json` under `files_dir`.
    ///
    /// # Arguments
    ///
    /// * `files_dir` - Bookclerk files directory that owns `plugin-grants.json`.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError`] when serialization or the write fails.
    pub fn save(&self, files_dir: &Path) -> Result<()> {
        let path = Self::path(files_dir);
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Returns the grant for `plugin_id`, if one is stored.
    ///
    /// # Arguments
    ///
    /// * `plugin_id` - Plugin id to look up.
    ///
    /// # Returns
    ///
    /// A reference to the matching [`PluginGrant`], or `None`.
    pub fn get(&self, plugin_id: &str) -> Option<&PluginGrant> {
        self.grants.iter().find(|g| g.plugin_id == plugin_id)
    }

    /// Inserts or replaces the grant for `grant.plugin_id`.
    ///
    /// # Arguments
    ///
    /// * `grant` - Full consent snapshot to persist in memory (call [`Self::save`] to flush).
    pub fn upsert(&mut self, grant: PluginGrant) {
        if let Some(existing) = self
            .grants
            .iter_mut()
            .find(|g| g.plugin_id == grant.plugin_id)
        {
            *existing = grant;
        } else {
            self.grants.push(grant);
        }
    }
}

/// Build the consent request surface from a manifest.
#[must_use]
pub fn consent_request(manifest: &PluginManifest) -> PluginGrant {
    let mut bindings = BTreeSet::new();
    let b = &manifest.capabilities.bindings;
    if b.config {
        bindings.insert("config".into());
    }
    if b.secrets {
        bindings.insert("secrets".into());
    }
    if b.plugin_kv {
        bindings.insert("plugin_kv".into());
    }
    if b.work_fs {
        bindings.insert("work_fs".into());
    }
    if b.oauth {
        bindings.insert("oauth".into());
    }
    let flags = manifest
        .workerd
        .as_ref()
        .map(|w| w.compatibility_flags.iter().cloned().collect())
        .unwrap_or_default();
    let domains: BTreeSet<String> = bookclerk_plugin_manifest::consent_domains_for(manifest)
        .unwrap_or_else(|_| {
            manifest
                .capabilities
                .network
                .domains
                .iter()
                .filter_map(|d| bookclerk_plugin_manifest::normalize_domain_pattern(d))
                .collect::<Vec<_>>()
        })
        .into_iter()
        .collect();
    let (cpu_ms, subrequests) = if manifest.runtime == PluginRuntimeKind::Workerd {
        let effective = manifest
            .workerd
            .as_ref()
            .map(|workerd| workerd.limits.clone())
            .unwrap_or_default()
            .effective();
        (Some(effective.cpu_ms), Some(effective.subrequests))
    } else {
        (None, None)
    };
    PluginGrant {
        plugin_id: manifest.id.clone(),
        kind: manifest.kind.as_str().to_string(),
        network_mode: match manifest.capabilities.network.mode {
            crate::manifest::NetworkMode::Deny => "deny".into(),
            crate::manifest::NetworkMode::Outbound => "outbound".into(),
        },
        domains,
        bindings,
        compatibility_flags: flags,
        cpu_ms,
        subrequests,
        approved_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// Human-readable consent lines for CLI/UI.
#[must_use]
pub fn consent_summary(grant: &PluginGrant) -> Vec<String> {
    let mut lines = vec![
        format!("Plugin: {} ({})", grant.plugin_id, grant.kind),
        format!("Network: {}", grant.network_mode),
    ];
    if grant.network_mode == "outbound" && grant.domains.is_empty() {
        lines.push(
            "WARNING: Native outbound has NO hostname allowlist — the jail permits \
             general internet access (TCP/UDP). Prefer workerd when you need domain \
             filtering."
                .into(),
        );
    }
    if !grant.domains.is_empty() {
        lines.push(format!(
            "Workerd initial outbound domains: {}",
            grant.domains.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
        lines
            .push("Redirect hops after an allowed initial host do not require re-approval.".into());
        let pyodide = bookclerk_plugin_manifest::PYODIDE_EGRESS_HOSTS;
        let has_pyodide = pyodide
            .iter()
            .any(|h| grant.domains.iter().any(|d| d.eq_ignore_ascii_case(h)));
        if has_pyodide {
            lines.push(format!(
                "Python runtime hosts (Pyodide/CDN): {}",
                pyodide.join(", ")
            ));
        }
    }
    if !grant.bindings.is_empty() {
        lines.push(format!(
            "Host bindings: {}",
            grant
                .bindings
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !grant.compatibility_flags.is_empty() {
        lines.push(format!(
            "Compatibility flags: {}",
            grant
                .compatibility_flags
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(cpu_ms) = grant.cpu_ms {
        lines.push(format!("Workerd CPU limit: {cpu_ms} ms"));
    }
    if let Some(subrequests) = grant.subrequests {
        lines.push(format!("Workerd subrequest limit: {subrequests}"));
    }
    lines
}

/// Returns true when a stored network mode can satisfy a requested network mode.
///
/// # Arguments
///
/// * `existing` - Network mode already approved by the operator.
/// * `requested` - Network mode requested by the current plugin manifest.
///
/// # Returns
///
/// `true` when the modes match, or when an existing `deny` grant is stricter
/// than a requested `outbound` grant.
#[must_use]
pub fn network_compatible(existing: &str, requested: &str) -> bool {
    existing.eq_ignore_ascii_case(requested)
        || (existing.eq_ignore_ascii_case("deny") && requested.eq_ignore_ascii_case("outbound"))
}

/// True when an existing grant stays within the manifest's current ceiling.
///
/// # Arguments
///
/// * `existing` - Stored grant to test.
/// * `requested` - Current consent request generated from the manifest.
///
/// # Returns
///
/// `true` when `existing` is a subset of `requested` for capabilities, with
/// `deny` accepted as a stricter version of requested `outbound`.
#[must_use]
pub fn grant_within_ceiling(existing: &PluginGrant, requested: &PluginGrant) -> bool {
    network_compatible(&existing.network_mode, &requested.network_mode)
        && existing.domains.is_subset(&requested.domains)
        && existing.bindings.is_subset(&requested.bindings)
        && existing
            .compatibility_flags
            .is_subset(&requested.compatibility_flags)
}

/// True when an existing grant is usable for the manifest's current request.
///
/// Kept for existing callers; consent is now a within-ceiling check, not a
/// superset coverage check.
#[must_use]
pub fn grant_covers(existing: &PluginGrant, requested: &PluginGrant) -> bool {
    grant_within_ceiling(existing, requested)
}

/// Spawn/delivery grant limited to both the stored approval and current request.
///
/// Intersects domains, bindings, and compatibility flags while preserving
/// identity and `approved_at` from `existing`.
#[must_use]
pub fn effective_grant(existing: &PluginGrant, requested: &PluginGrant) -> PluginGrant {
    let domains = existing
        .domains
        .intersection(&requested.domains)
        .cloned()
        .collect();
    let bindings = existing
        .bindings
        .intersection(&requested.bindings)
        .cloned()
        .collect();
    let compatibility_flags = existing
        .compatibility_flags
        .intersection(&requested.compatibility_flags)
        .cloned()
        .collect();
    PluginGrant {
        plugin_id: existing.plugin_id.clone(),
        kind: existing.kind.clone(),
        network_mode: if existing.network_mode.eq_ignore_ascii_case("deny")
            && requested.network_mode.eq_ignore_ascii_case("outbound")
        {
            existing.network_mode.clone()
        } else {
            requested.network_mode.clone()
        },
        domains,
        bindings,
        compatibility_flags,
        cpu_ms: normalize_cpu_ms(existing.cpu_ms.or(requested.cpu_ms)),
        subrequests: normalize_subrequests(existing.subrequests.or(requested.subrequests)),
        approved_at: existing.approved_at.clone(),
    }
}

/// Validates and normalizes an operator-supplied grant against a manifest ceiling.
///
/// # Arguments
///
/// * `approved` - Operator-supplied grant draft.
/// * `ceiling` - Full consent request generated from the current manifest.
///
/// # Returns
///
/// A normalized grant with manifest identity, clamped workerd limits, and a fresh
/// `approved_at` timestamp.
///
/// # Errors
///
/// Returns [`PluginError`] when the requested grant widens beyond `ceiling`.
pub fn validate_approved_grant(
    approved: &PluginGrant,
    ceiling: &PluginGrant,
) -> Result<PluginGrant> {
    if !approved.plugin_id.is_empty() && approved.plugin_id != ceiling.plugin_id {
        return Err(PluginError::message(format!(
            "grant plugin id `{}` does not match `{}`",
            approved.plugin_id, ceiling.plugin_id
        )));
    }
    if !approved.kind.is_empty() && approved.kind != ceiling.kind {
        return Err(PluginError::message(format!(
            "grant kind `{}` does not match `{}`",
            approved.kind, ceiling.kind
        )));
    }
    if !grant_within_ceiling(approved, ceiling) {
        return Err(PluginError::message(format!(
            "grant exceeds current plugin capabilities; re-approve with `bookclerk plugins approve {}`",
            ceiling.plugin_id
        )));
    }
    let cpu_ms = normalize_limit_with_ceiling(approved.cpu_ms, ceiling.cpu_ms, true)?;
    let subrequests =
        normalize_limit_with_ceiling(approved.subrequests, ceiling.subrequests, false)?;
    Ok(PluginGrant {
        plugin_id: ceiling.plugin_id.clone(),
        kind: ceiling.kind.clone(),
        network_mode: if approved.network_mode.is_empty() {
            ceiling.network_mode.clone()
        } else {
            approved.network_mode.clone()
        },
        domains: approved.domains.clone(),
        bindings: approved.bindings.clone(),
        compatibility_flags: approved.compatibility_flags.clone(),
        cpu_ms,
        subrequests,
        approved_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn normalize_cpu_ms(value: Option<u32>) -> Option<u32> {
    value.map(|cpu_ms| {
        WorkerdLimits {
            cpu_ms: Some(cpu_ms),
            subrequests: None,
        }
        .effective()
        .cpu_ms
    })
}

fn normalize_subrequests(value: Option<u32>) -> Option<u32> {
    value.map(|subrequests| {
        WorkerdLimits {
            cpu_ms: None,
            subrequests: Some(subrequests),
        }
        .effective()
        .subrequests
    })
}

fn normalize_limit_with_ceiling(
    approved: Option<u32>,
    ceiling: Option<u32>,
    cpu: bool,
) -> Result<Option<u32>> {
    let raw = match (approved, ceiling) {
        (Some(_), None) => {
            return Err(PluginError::message(
                "grant exceeds current plugin workerd limits",
            ))
        }
        (Some(raw), Some(_)) | (None, Some(raw)) => raw,
        (None, None) => return Ok(None),
    };
    let normalized = if cpu {
        normalize_cpu_ms(Some(raw)).expect("Some input returns Some")
    } else {
        normalize_subrequests(Some(raw)).expect("Some input returns Some")
    };
    if let Some(ceiling) = ceiling {
        let normalized_ceiling = if cpu {
            normalize_cpu_ms(Some(ceiling)).expect("Some input returns Some")
        } else {
            normalize_subrequests(Some(ceiling)).expect("Some input returns Some")
        };
        if normalized > normalized_ceiling {
            return Err(PluginError::message(
                "grant exceeds current plugin workerd limits",
            ));
        }
    }
    Ok(Some(normalized))
}

/// Require a covering grant before enable **or** every external spawn.
///
/// # Arguments
///
/// * `files_dir` - Bookclerk files directory containing `plugin-grants.json`.
/// * `manifest` - Plugin manifest whose requested capabilities must be covered.
///
/// # Returns
///
/// Effective [`PluginGrant`] capped to the current request surface.
///
/// # Errors
///
/// Returns [`PluginError`] when no grant exists or it exceeds current capabilities.
pub fn require_grant(files_dir: &Path, manifest: &PluginManifest) -> Result<PluginGrant> {
    let store = PluginGrantStore::load(files_dir)?;
    let requested = consent_request(manifest);
    match store.get(&manifest.id) {
        Some(existing) if grant_within_ceiling(existing, &requested) => {
            Ok(effective_grant(existing, &requested))
        }
        Some(_) => Err(PluginError::message(format!(
            "plugin `{}` grant exceeds current plugin capabilities; re-approve with `bookclerk plugins approve {}`",
            manifest.id, manifest.id
        ))),
        None => Err(PluginError::message(format!(
            "plugin `{}` has no permission grant; run `bookclerk plugins approve {}` first",
            manifest.id, manifest.id
        ))),
    }
}

/// True when `grant.bindings` contains `name` (case-insensitive).
#[must_use]
pub fn grant_has_binding(grant: &PluginGrant, name: &str) -> bool {
    grant
        .bindings
        .iter()
        .any(|binding| binding.eq_ignore_ascii_case(name))
}

/// Fail closed when a delivery site needs a binding the covering grant lacks.
pub fn require_binding(grant: &PluginGrant, name: &str) -> Result<()> {
    if grant_has_binding(grant, name) {
        Ok(())
    } else {
        Err(PluginError::message(format!(
            "plugin `{}` grant lacks binding `{name}`; re-approve with `bookclerk plugins approve {}`",
            grant.plugin_id, grant.plugin_id
        )))
    }
}

/// Handshake `config` payload: non-empty settings only when the grant includes `config`.
#[must_use]
pub fn handshake_config_for_grant(
    grant: &PluginGrant,
    config_table: serde_json::Value,
) -> serde_json::Value {
    if grant_has_binding(grant, "config") {
        config_table
    } else {
        serde_json::json!({})
    }
}

/// Platform guests shipped with the installer (`sqlite`, `local`) skip the
/// consent UX and are enabled by default. Persist a covering grant when the
/// installed manifest stays within the safe platform envelope (deny network,
/// only `config` / `work_fs` bindings).
pub fn ensure_platform_grant(files_dir: &Path, manifest: &PluginManifest) -> Result<PluginGrant> {
    if !is_platform_plugin_id(&manifest.id) {
        return require_grant(files_dir, manifest);
    }
    let requested = consent_request(manifest);
    if !is_safe_platform_request(&requested) {
        return Err(PluginError::message(format!(
            "platform plugin `{}` declares capabilities outside the installer envelope; \
             run `bookclerk plugins approve {}`",
            manifest.id, manifest.id
        )));
    }
    let mut store = PluginGrantStore::load(files_dir)?;
    match store.get(&manifest.id) {
        Some(existing) if grant_within_ceiling(existing, &requested) => {
            Ok(effective_grant(existing, &requested))
        }
        Some(_) | None => {
            store.upsert(requested.clone());
            store.save(files_dir)?;
            Ok(requested)
        }
    }
}

/// Resolve the covering grant for spawn: platform auto-grant, else [`require_grant`].
pub fn spawn_grant(files_dir: &Path, manifest: &PluginManifest) -> Result<PluginGrant> {
    if is_platform_plugin_id(&manifest.id) {
        ensure_platform_grant(files_dir, manifest)
    } else {
        require_grant(files_dir, manifest)
    }
}

/// Returns true for installer platform guests (`sqlite`, `local`).
///
/// # Arguments
///
/// * `id` - Plugin id to test (case-insensitive).
///
/// # Returns
///
/// `true` when the id is a built-in platform plugin.
#[must_use]
pub fn is_platform_plugin_id(id: &str) -> bool {
    matches!(id.to_ascii_lowercase().as_str(), "sqlite" | "local")
}

fn is_safe_platform_request(grant: &PluginGrant) -> bool {
    grant.network_mode == "deny"
        && grant.domains.is_empty()
        && grant.compatibility_flags.is_empty()
        && grant
            .bindings
            .iter()
            .all(|b| b == "config" || b == "work_fs")
}

/// Reject handshake claims that exceed the manifest (and covering grant).
pub fn validate_handshake_capabilities(
    manifest: &PluginManifest,
    grant: &PluginGrant,
    capabilities: &[String],
    portal_auth_mode: Option<&str>,
) -> Result<()> {
    let oauth_mode = portal_auth_mode.is_some_and(|m| m.eq_ignore_ascii_case("oauth"));
    let oauth_methods = capabilities.iter().any(|cap| {
        cap.eq_ignore_ascii_case("loginStart") || cap.eq_ignore_ascii_case("loginComplete")
    });
    if oauth_mode || oauth_methods {
        if !manifest.capabilities.bindings.oauth {
            return Err(PluginError::message(format!(
                "plugin `{}` handshake advertises OAuth without bindings.oauth in plugin.toml",
                manifest.id
            )));
        }
        require_binding(grant, "oauth")?;
    }

    // Lifecycle / entrypoint methods are always implied; kind-specific surfaces
    // in `capabilities.methods` are what consent is meant to bound.
    const CORE_CAPS: &[&str] = &[
        "handshake",
        "shutdown",
        "health",
        "diagnose",
        "start",
        "onEvent",
        "pollEvents",
        "cli",
        "cliDescribe",
        "cliInvoke",
    ];
    let declared = &manifest.capabilities.methods.list;
    if declared.is_empty() {
        return Ok(());
    }
    for cap in capabilities {
        if CORE_CAPS.iter().any(|name| name.eq_ignore_ascii_case(cap)) {
            continue;
        }
        if !declared.iter().any(|name| name.eq_ignore_ascii_case(cap)) {
            return Err(PluginError::message(format!(
                "plugin `{}` handshake advertises capability `{cap}` not listed in \
                 capabilities.methods",
                manifest.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    fn sample_grant(domains: &[&str], bindings: &[&str], flags: &[&str]) -> PluginGrant {
        PluginGrant {
            plugin_id: "demo".into(),
            kind: "source".into(),
            network_mode: "outbound".into(),
            domains: domains.iter().map(|s| (*s).to_string()).collect(),
            bindings: bindings.iter().map(|s| (*s).to_string()).collect(),
            compatibility_flags: flags.iter().map(|s| (*s).to_string()).collect(),
            cpu_ms: None,
            subrequests: None,
            approved_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn grant_covers_allows_operator_domain_subset() {
        let existing = sample_grant(&["a.example"], &["config"], &[]);
        let requested = sample_grant(&["a.example", "b.example"], &["config"], &[]);
        assert!(grant_covers(&existing, &requested));
    }

    #[test]
    fn grant_covers_allows_operator_binding_subset() {
        let existing = sample_grant(&["a.example"], &["config"], &[]);
        let requested = sample_grant(&["a.example"], &["config", "secrets"], &[]);
        assert!(grant_covers(&existing, &requested));
    }

    #[test]
    fn grant_covers_allows_operator_flag_subset() {
        let existing = sample_grant(&[], &[], &["nodejs_compat"]);
        let requested = sample_grant(&[], &[], &["nodejs_compat", "streams_enable_constructors"]);
        assert!(grant_covers(&existing, &requested));
    }

    #[test]
    fn grant_covers_rejects_stored_capabilities_beyond_ceiling() {
        let existing = sample_grant(
            &["a.example", "b.example"],
            &["config", "secrets"],
            &["nodejs_compat"],
        );
        let requested = sample_grant(&["a.example"], &["config"], &[]);
        assert!(!grant_covers(&existing, &requested));
    }

    #[test]
    fn grant_covers_allows_deny_when_request_is_outbound() {
        let mut existing = sample_grant(&[], &[], &[]);
        existing.network_mode = "deny".into();
        let requested = sample_grant(&[], &[], &[]);
        assert!(grant_covers(&existing, &requested));
    }

    #[test]
    fn validate_approved_grant_rejects_domain_widen() {
        let ceiling = sample_grant(&["a.example"], &["config"], &[]);
        let approved = sample_grant(&["a.example", "b.example"], &["config"], &[]);
        let err = validate_approved_grant(&approved, &ceiling)
            .unwrap_err()
            .to_string();
        assert!(err.contains("exceeds"), "{err}");
    }

    #[test]
    fn validate_approved_grant_clamps_workerd_limits() {
        let mut ceiling = sample_grant(&["a.example"], &["config"], &["nodejs_compat"]);
        ceiling.cpu_ms = Some(WorkerdLimits::MAX_CPU_MS);
        ceiling.subrequests = Some(50);
        let mut approved = sample_grant(&["a.example"], &["config"], &["nodejs_compat"]);
        approved.cpu_ms = Some(WorkerdLimits::MAX_CPU_MS + 50);
        approved.subrequests = Some(40);
        let grant = validate_approved_grant(&approved, &ceiling).unwrap();
        assert_eq!(grant.cpu_ms, Some(WorkerdLimits::MAX_CPU_MS));
        assert_eq!(grant.subrequests, Some(40));
        assert!(!grant.approved_at.is_empty());
    }

    #[test]
    fn grant_has_binding_is_case_insensitive() {
        let grant = sample_grant(&[], &["config", "Secrets"], &[]);
        assert!(grant_has_binding(&grant, "config"));
        assert!(grant_has_binding(&grant, "CONFIG"));
        assert!(grant_has_binding(&grant, "secrets"));
        assert!(!grant_has_binding(&grant, "oauth"));
    }

    #[test]
    fn require_binding_fails_closed() {
        let grant = sample_grant(&[], &["config"], &[]);
        assert!(require_binding(&grant, "config").is_ok());
        let err = require_binding(&grant, "secrets").unwrap_err().to_string();
        assert!(err.contains("lacks binding `secrets`"), "{err}");
    }

    #[test]
    fn handshake_config_omitted_without_config_binding() {
        let grant = sample_grant(&[], &["secrets"], &[]);
        let delivered = handshake_config_for_grant(
            &grant,
            serde_json::json!({ "greeting": "hi", "enabled": true }),
        );
        assert_eq!(delivered, serde_json::json!({}));
        let with_config = sample_grant(&[], &["config"], &[]);
        let kept =
            handshake_config_for_grant(&with_config, serde_json::json!({ "greeting": "hi" }));
        assert_eq!(kept["greeting"], "hi");
    }

    #[test]
    fn require_grant_fails_without_store_entry() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "demo"
kind = "source"
runtime = "native"
command = "./demo"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
"#,
        )
        .unwrap();
        let err = require_grant(dir.path(), &manifest)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no permission grant"), "{err}");
    }

    #[test]
    fn require_grant_succeeds_when_manifest_widens_past_stored_subset() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PluginGrantStore::default();
        let mut existing = sample_grant(&[], &["config"], &[]);
        existing.network_mode = "deny".into();
        store.upsert(existing);
        store.save(dir.path()).unwrap();

        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "demo"
kind = "source"
runtime = "native"
command = "./demo"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
secrets = true
"#,
        )
        .unwrap();
        let grant = require_grant(dir.path(), &manifest).unwrap();
        assert!(grant_has_binding(&grant, "config"));
        assert!(!grant_has_binding(&grant, "secrets"));
    }

    #[test]
    fn require_grant_fails_when_stored_domains_exceed_current_request() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PluginGrantStore::default();
        let mut existing = sample_grant(&["old.example"], &["config"], &[]);
        existing.approved_at = "2026-01-01T00:00:00Z".into();
        store.upsert(existing);
        store.save(dir.path()).unwrap();

        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "demo"
kind = "source"
runtime = "workerd"

[workerd]
compatibility_date = "2026-08-01"
main_module = "index.js"

[capabilities.network]
mode = "outbound"
domains = ["api.example.com"]

[capabilities.bindings]
config = true
"#,
        )
        .unwrap();
        let err = require_grant(dir.path(), &manifest)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("grant exceeds current plugin capabilities"),
            "{err}"
        );
    }

    #[test]
    fn platform_grant_auto_persists_for_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "sqlite"
kind = "database"
runtime = "native"
command = "./sqlite"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
work_fs = true
"#,
        )
        .unwrap();
        let grant = ensure_platform_grant(dir.path(), &manifest).unwrap();
        assert!(grant_has_binding(&grant, "config"));
        assert!(grant_has_binding(&grant, "work_fs"));
        // Second call returns an effective grant (same surface, preserved approved_at).
        let again = spawn_grant(dir.path(), &manifest).unwrap();
        assert_eq!(again.plugin_id, "sqlite");
        assert_eq!(again.approved_at, grant.approved_at);
        assert_eq!(again.bindings, grant.bindings);
    }

    #[test]
    fn platform_grant_replaces_stale_grant_when_manifest_narrows() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PluginGrantStore::default();
        let mut existing = sample_grant(&[], &["config", "work_fs"], &[]);
        existing.plugin_id = "sqlite".into();
        existing.kind = "database".into();
        existing.network_mode = "deny".into();
        existing.approved_at = "2026-02-02T00:00:00Z".into();
        store.upsert(existing);
        store.save(dir.path()).unwrap();

        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "sqlite"
kind = "database"
runtime = "native"
command = "./sqlite"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
"#,
        )
        .unwrap();
        let grant = ensure_platform_grant(dir.path(), &manifest).unwrap();
        assert_eq!(
            grant.bindings.iter().cloned().collect::<Vec<_>>(),
            vec!["config".to_string()]
        );
        assert!(!grant_has_binding(&grant, "work_fs"));
    }

    #[test]
    fn platform_grant_fails_outside_installer_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PluginGrantStore::default();
        let mut existing = sample_grant(&[], &["config"], &[]);
        existing.plugin_id = "sqlite".into();
        existing.kind = "database".into();
        existing.network_mode = "deny".into();
        store.upsert(existing);
        store.save(dir.path()).unwrap();

        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "sqlite"
kind = "database"
runtime = "native"
command = "./sqlite"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
secrets = true
"#,
        )
        .unwrap();
        let err = ensure_platform_grant(dir.path(), &manifest)
            .unwrap_err()
            .to_string();
        assert!(err.contains("outside the installer envelope"), "{err}");
    }

    #[test]
    fn effective_grant_preserves_approval_and_intersects_request_surface() {
        let existing = sample_grant(
            &["a.example", "b.example"],
            &["config", "secrets"],
            &["nodejs_compat"],
        );
        let requested = sample_grant(&["a.example"], &["config"], &[]);
        let effective = effective_grant(&existing, &requested);
        assert_eq!(effective.plugin_id, existing.plugin_id);
        assert_eq!(effective.kind, existing.kind);
        assert_eq!(effective.approved_at, existing.approved_at);
        assert_eq!(effective.network_mode, requested.network_mode);
        assert_eq!(effective.domains, requested.domains);
        assert_eq!(effective.bindings, requested.bindings);
        assert_eq!(effective.compatibility_flags, requested.compatibility_flags);
    }

    #[test]
    fn require_grant_keeps_stored_subset_after_repeated_manifest_widening() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PluginGrantStore::default();
        let mut existing = sample_grant(&[], &["config"], &[]);
        existing.network_mode = "deny".into();
        store.upsert(existing);
        store.save(dir.path()).unwrap();

        let narrowed = PluginManifest::parse(
            r#"
api_version = 1
id = "demo"
kind = "source"
runtime = "native"
command = "./demo"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
"#,
        )
        .unwrap();
        let effective = require_grant(dir.path(), &narrowed).unwrap();
        assert!(grant_has_binding(&effective, "config"));

        let widened = PluginManifest::parse(
            r#"
api_version = 1
id = "demo"
kind = "source"
runtime = "native"
command = "./demo"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
plugin_kv = true
"#,
        )
        .unwrap();
        let effective = require_grant(dir.path(), &widened).unwrap();
        assert!(grant_has_binding(&effective, "config"));
        assert!(!grant_has_binding(&effective, "plugin_kv"));
    }

    #[test]
    fn validate_approved_grant_rejects_widen_beyond_ceiling() {
        let ceiling = sample_grant(&["api.example.com"], &["config"], &["nodejs_compat"]);
        let approved = sample_grant(
            &["api.example.com", "extra.example.com"],
            &["config"],
            &["nodejs_compat"],
        );
        let err = validate_approved_grant(&approved, &ceiling)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("grant exceeds current plugin capabilities"),
            "{err}"
        );
    }

    #[test]
    fn validate_approved_grant_normalizes_limits_and_preserves_subset() {
        let mut ceiling = sample_grant(&["api.example.com"], &["config", "secrets"], &[]);
        ceiling.cpu_ms = Some(60_000);
        ceiling.subrequests = Some(500);

        let mut approved = sample_grant(&["api.example.com"], &["config"], &[]);
        approved.cpu_ms = Some(15_000);
        approved.subrequests = Some(25);

        let normalized = validate_approved_grant(&approved, &ceiling).unwrap();
        assert_eq!(normalized.plugin_id, ceiling.plugin_id);
        assert_eq!(normalized.kind, ceiling.kind);
        assert_eq!(normalized.cpu_ms, Some(15_000));
        assert_eq!(normalized.subrequests, Some(25));
        assert!(grant_has_binding(&normalized, "config"));
        assert!(!grant_has_binding(&normalized, "secrets"));
        assert_ne!(normalized.approved_at, approved.approved_at);
    }

    #[test]
    fn validate_approved_grant_rejects_limits_above_ceiling() {
        let mut ceiling = sample_grant(&[], &[], &[]);
        ceiling.cpu_ms = Some(30_000);
        ceiling.subrequests = Some(50);
        let mut approved = ceiling.clone();
        approved.cpu_ms = Some(60_000);
        let err = validate_approved_grant(&approved, &ceiling)
            .unwrap_err()
            .to_string();
        assert!(err.contains("workerd limits"), "{err}");
    }

    #[test]
    fn validate_handshake_rejects_oauth_without_binding() {
        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "demo"
kind = "source"
runtime = "native"
command = "./demo"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
"#,
        )
        .unwrap();
        let grant = consent_request(&manifest);
        let err = validate_handshake_capabilities(
            &manifest,
            &grant,
            &["loginStart".into()],
            Some("oauth"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("OAuth"), "{err}");
    }

    #[test]
    fn validate_handshake_rejects_undeclared_methods() {
        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "demo"
kind = "integration"
runtime = "native"
command = "./demo"

[capabilities.network]
mode = "deny"

[capabilities.methods]
list = ["handshake", "health"]
"#,
        )
        .unwrap();
        let grant = consent_request(&manifest);
        let err = validate_handshake_capabilities(
            &manifest,
            &grant,
            &["handshake".into(), "scanLibrary".into()],
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("scanLibrary"), "{err}");
    }

    #[test]
    fn validate_handshake_allows_core_entrypoint_methods() {
        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "demo"
kind = "integration"
runtime = "native"
command = "./demo"

[capabilities.network]
mode = "deny"

[capabilities.methods]
list = ["handshake", "health", "diagnose", "onEvent", "cli"]
"#,
        )
        .unwrap();
        let grant = consent_request(&manifest);
        validate_handshake_capabilities(
            &manifest,
            &grant,
            &[
                "handshake".into(),
                "health".into(),
                "start".into(),
                "cli".into(),
            ],
            None,
        )
        .unwrap();
    }

    #[test]
    fn python_outbound_consent_includes_pyodide_hosts() {
        let manifest = PluginManifest::parse(
            r#"
api_version = 1
id = "echo_workerd_python"
kind = "integration"
runtime = "workerd"

[workerd]
compatibility_date = "2026-08-01"
compatibility_flags = ["python_workers"]
main_module = "plugin.py"

[[modules]]
name = "plugin.py"
path = "plugin.py"
type = "python"

[capabilities.network]
mode = "outbound"
domains = ["api.example.com"]

[capabilities.bindings]
config = true
"#,
        )
        .unwrap();
        let grant = consent_request(&manifest);
        assert!(grant.domains.contains("api.example.com"));
        for host in bookclerk_plugin_manifest::PYODIDE_EGRESS_HOSTS {
            assert!(
                grant.domains.iter().any(|d| d == *host),
                "missing Pyodide host {host} in {:?}",
                grant.domains
            );
        }
        let summary = consent_summary(&grant);
        assert!(
            summary.iter().any(|l| l.contains("Python runtime hosts")),
            "{summary:?}"
        );
        assert_eq!(grant.cpu_ms, Some(WorkerdLimits::DEFAULT_CPU_MS));
        assert_eq!(grant.subrequests, Some(WorkerdLimits::DEFAULT_SUBREQUESTS));
        assert!(
            summary.iter().any(|l| l.contains("Workerd CPU limit")),
            "{summary:?}"
        );
        assert!(
            summary
                .iter()
                .any(|l| l.contains("Workerd subrequest limit")),
            "{summary:?}"
        );
        // Grant that only has author domains remains usable as a stored subset.
        let narrow = sample_grant(&["api.example.com"], &["config"], &["python_workers"]);
        assert!(grant_covers(&narrow, &grant));
    }

    #[test]
    fn cpu_ms_and_subrequests_round_trip_on_grant() {
        let mut grant = sample_grant(&[], &[], &[]);
        grant.cpu_ms = Some(12_000);
        grant.subrequests = Some(75);
        let encoded = serde_json::to_string(&grant).unwrap();
        assert!(encoded.contains("cpuMs"));
        assert!(encoded.contains("subrequests"));
        let decoded: PluginGrant = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.cpu_ms, Some(12_000));
        assert_eq!(decoded.subrequests, Some(75));
    }
}
