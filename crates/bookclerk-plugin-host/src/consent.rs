//! Operator permission grants for plugin capabilities.
//!
//! Structural authority (entrypoints, producers, host bindings, named
//! databases, jobs) originates in the plugin package. The operator may
//! **narrow** it but cannot invent a structural capability the manifest did
//! not declare.
//!
//! Network authority is operator-extensible: the operator may add hostnames
//! the author omitted. Guest `describe()` remains refinement only.
//!
//! ```text
//! effective structural = manifest ∩ operator grant ∩ host policy
//! effective network    = host policy ∩ approved(manifest network ∪ operator additions)
//! ```

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use bookclerk_config::{resolve_postgres_url, Config, DatabasePluginKind};
use bookclerk_plugin_abi::{PluginCapabilities, PortalAuthMode};
use bookclerk_plugin_manifest::{
    is_public_internet, is_restricted_hostname, normalize_domain_pattern, EgressPolicy,
    NetworkMode, TcpGrant,
};

use crate::manifest::{PluginManifest, PluginRuntimeKind, WorkerdLimits};
use crate::spawn_plan::GuestRuntimeKind;
use crate::{PluginError, Result};

/// Filename under `$BOOKCLERK_FILES_DIR` for persisted grants.
pub const GRANTS_FILE: &str = "plugin-grants.json";

/// Env keys consumed by `bookclerk-workerd` (`grant.rs`) at isolate start.
pub const WORKERD_GRANT_NETWORK_MODE_ENV: &str = "BOOKCLERK_WORKERD_GRANT_NETWORK_MODE";
/// Comma-separated grant domain allowlist for workerd egress.
pub const WORKERD_GRANT_DOMAINS_ENV: &str = "BOOKCLERK_WORKERD_GRANT_DOMAINS";
/// Grant CPU budget (ms) for workerd logging / limit narrowing.
pub const WORKERD_GRANT_CPU_MS_ENV: &str = "BOOKCLERK_WORKERD_GRANT_CPU_MS";
/// Grant subrequest budget injected into workerd `EGRESS_POLICY`.
pub const WORKERD_GRANT_SUBREQUESTS_ENV: &str = "BOOKCLERK_WORKERD_GRANT_SUBREQUESTS";
/// Canonical JSON [`EgressPolicy`] (preferred over piecemeal domain env vars).
pub const WORKERD_GRANT_POLICY_ENV: &str = "BOOKCLERK_WORKERD_GRANT_POLICY";

/// Default per-plugin `data/` and `tmp/` disk budget (MiB each).
pub const PLUGIN_STATE_BUDGET_MIB_DEFAULT: u32 = 512;
/// Host hard cap for per-plugin disk budget overrides (MiB).
pub const PLUGIN_STATE_BUDGET_MIB_MAX: u32 = 4096;

/// Default jail Spec memory ceiling (MiB) for confined guests.
pub const PLUGIN_JAIL_MEMORY_MIB_DEFAULT: u32 = 512;
/// Host hard cap for per-plugin jail memory overrides (MiB).
pub const PLUGIN_JAIL_MEMORY_MIB_MAX: u32 = 4096;
/// Default jail Spec CPU rate percent for confined guests (one-core units).
pub const PLUGIN_JAIL_CPU_RATE_DEFAULT: u32 = 80;
/// Absolute ceiling used only when host CPU detection is unavailable in docs.
///
/// Prefer [`host_cpu_rate_max`] at runtime (`logical_cpus × 100`).
pub const PLUGIN_JAIL_CPU_RATE_MAX: u32 = 100;
/// Default jail CPU as cores (`PLUGIN_JAIL_CPU_RATE_DEFAULT` / 100).
pub const PLUGIN_JAIL_CPU_CORES_DEFAULT: f64 = 0.80;
/// Default **extra** processes/threads beyond launcher overhead.
pub const PLUGIN_JAIL_EXTRA_PROCESSES_DEFAULT: u32 = 2;
/// Host hard cap for per-plugin extra process budget.
pub const PLUGIN_JAIL_EXTRA_PROCESSES_MAX: u32 = 62;
/// Absolute OS jail PID ceiling (`overhead + extra`, inclusive).
pub const PLUGIN_JAIL_ACTIVE_PROCESSES_MAX: u32 = 64;

/// Fixed jail occupancy for the launcher / primary guest tree.
///
/// Workerd isolate: `bookclerk-workerd` plus the `workerd` child (2 PIDs).
/// Native behind workerd (the product path for `runtime = "native"`): launcher,
/// `workerd`, and the native guest (3 PIDs). Direct native (diagnostic
/// transport): `bookclerk-jail` execs the guest (1 PID).
#[must_use]
pub fn jail_process_overhead(runtime: GuestRuntimeKind) -> u32 {
    runtime.process_overhead()
}

/// Clamp an operator/manifest extra-process budget (`0..=`[`PLUGIN_JAIL_EXTRA_PROCESSES_MAX`]).
#[must_use]
pub fn effective_extra_processes(value: Option<u32>) -> u32 {
    value
        .unwrap_or(PLUGIN_JAIL_EXTRA_PROCESSES_DEFAULT)
        .min(PLUGIN_JAIL_EXTRA_PROCESSES_MAX)
}

/// Absolute Spec `active_processes` from runtime overhead + extra budget.
#[must_use]
pub fn active_processes_for(runtime: GuestRuntimeKind, extra: u32) -> u32 {
    let overhead = jail_process_overhead(runtime);
    let extra = extra.min(PLUGIN_JAIL_EXTRA_PROCESSES_MAX);
    (overhead.saturating_add(extra)).min(PLUGIN_JAIL_ACTIVE_PROCESSES_MAX)
}

/// Convert Spec/config percent-of-one-core (100 = 1.00 core) to cores (2 d.p.).
#[must_use]
pub fn percent_to_cores(percent: u32) -> f64 {
    (f64::from(percent) / 100.0 * 100.0).round() / 100.0
}

/// Convert cores to Spec/config percent-of-one-core, rounded to 0.01 core.
#[must_use]
pub fn cores_to_percent(cores: f64) -> u32 {
    if !cores.is_finite() || cores <= 0.0 {
        return 0;
    }
    let pct = (cores * 100.0).round();
    if pct < 1.0 {
        1
    } else if pct > f64::from(u32::MAX) {
        u32::MAX
    } else {
        pct as u32
    }
}

/// Host max jail CPU in cores (`logical_cpus` as a 2 d.p. float).
#[must_use]
pub fn host_cpu_cores_max() -> f64 {
    percent_to_cores(host_cpu_rate_max())
}

/// Clamp operator/manifest cores into `0.01..=`[`host_cpu_cores_max`].
#[must_use]
pub fn effective_cpu_cores(value: Option<f64>) -> f64 {
    let max = host_cpu_cores_max();
    let raw = value.unwrap_or(PLUGIN_JAIL_CPU_CORES_DEFAULT);
    if !raw.is_finite() {
        return PLUGIN_JAIL_CPU_CORES_DEFAULT.min(max);
    }
    let stepped = (raw * 100.0).round() / 100.0;
    stepped.clamp(0.01, max)
}

/// Host binding names operators may grant (widen or narrow).
pub const KNOWN_HOST_BINDINGS: &[&str] = &["config", "secrets", "plugin_kv", "work_fs", "oauth"];

/// Grant-entry prefix for named plugin database bindings (`database:<NAME>`).
pub const DATABASE_BINDING_PREFIX: &str = "database:";

/// The binding name when a grant entry is a named database binding.
#[must_use]
pub fn database_binding_name(binding: &str) -> Option<&str> {
    binding.strip_prefix(DATABASE_BINDING_PREFIX)
}

/// Consented database binding names on a grant, in manifest-set order.
#[must_use]
pub fn granted_database_bindings(grant: &PluginGrant) -> Vec<String> {
    grant
        .bindings
        .iter()
        .filter_map(|b| database_binding_name(b).map(str::to_string))
        .collect()
}

/// One approved grant snapshot for a provenance-qualified plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginGrant {
    /// Canonical [`bookclerk_plugin_catalog::PluginKey`].
    #[serde(default)]
    pub plugin_key: String,
    /// Display / CLI alias from `plugin.toml` (not security authoritative).
    pub plugin_id: String,
    /// Approved named entrypoints (wire names: `storefront`, `storage`,
    /// `databaseAdapter`, `remoteLibrary`, `cli`, `oidc`).
    #[serde(default)]
    pub entrypoints: BTreeSet<String>,
    /// Approved event types the plugin may publish through its events binding.
    #[serde(default)]
    pub producers: BTreeSet<String>,
    /// Approved network mode: `deny` or `outbound`.
    pub network_mode: String,
    /// Approved initial outbound domain patterns (**workerd** allowlist).
    ///
    /// Effective set: manifest request ∪ operator additions − operator denials.
    pub domains: BTreeSet<String>,
    /// Network destinations requested by the installed manifest (not operator-added).
    #[serde(default)]
    pub manifest_domains: BTreeSet<String>,
    /// Operator-added destinations beyond the current manifest request.
    ///
    /// Survive upgrades of the same [`Self::plugin_key`]. Do not transfer across provenance.
    #[serde(default)]
    pub operator_added_domains: BTreeSet<String>,
    /// Operator-denied destinations (even if the manifest still lists them).
    #[serde(default)]
    pub operator_denied_domains: BTreeSet<String>,
    /// Effective raw TCP grants (manifest ∪ operator additions − denials).
    #[serde(default)]
    pub tcp: BTreeSet<TcpGrant>,
    /// TCP grants requested by the installed manifest.
    #[serde(default)]
    pub manifest_tcp: BTreeSet<TcpGrant>,
    /// Operator-added TCP grants; survive upgrades of the same PluginKey.
    #[serde(default)]
    pub operator_added_tcp: BTreeSet<TcpGrant>,
    /// Operator-denied TCP grants (even if the current manifest still lists them).
    ///
    /// Same PluginKey upgrades must not silently restore a TCP host the operator
    /// removed. Host spawn overlays (postgres URL, D1, ABS, S3) may still add
    /// destinations from operator config after this set is applied.
    #[serde(default)]
    pub operator_denied_tcp: BTreeSet<TcpGrant>,
    /// Effective CIDR grants beyond the public Internet.
    #[serde(default)]
    pub address_cidrs: BTreeSet<String>,
    /// CIDRs requested by the installed manifest.
    #[serde(default)]
    pub manifest_cidrs: BTreeSet<String>,
    /// Operator-added CIDRs; survive upgrades of the same PluginKey.
    #[serde(default)]
    pub operator_added_cidrs: BTreeSet<String>,
    /// Effective undeclared-public-redirect permission.
    #[serde(default)]
    pub allow_undeclared_public_redirects: bool,
    /// Explicit operator override for undeclared public redirects (`None` = follow manifest).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_allow_undeclared_public_redirects: Option<bool>,
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
    /// Optional per-plugin disk budget for `data/` and `tmp/` (MiB each).
    ///
    /// Applies to **native and workerd** guests. Unset →
    /// [`PLUGIN_STATE_BUDGET_MIB_DEFAULT`]; clamped to
    /// [`PLUGIN_STATE_BUDGET_MIB_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_mib: Option<u32>,
    /// Optional jail Spec memory ceiling (MiB) for **native and workerd**.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<u32>,
    /// Optional jail Spec CPU rate as percent of one logical CPU for **native**
    /// guests (`1..=`[`host_cpu_rate_max`]; values above 100 request multi-core
    /// bandwidth). Workerd guests omit this and use `cpu_ms` plus the host jail
    /// CPU ceiling instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_rate_percent: Option<u32>,
    /// Optional **extra** process/thread budget beyond launcher overhead (**native**).
    ///
    /// Workerd guests omit this; the host applies default extra headroom only.
    /// Absolute Spec `active_processes` = overhead(runtime) + this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_processes: Option<u32>,
    /// RFC 3339 time when the operator approved this grant.
    pub approved_at: String,
}

impl PluginGrant {
    /// Empty grant used as a struct-update base for tests and new fields.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            plugin_key: String::new(),
            plugin_id: String::new(),
            entrypoints: BTreeSet::new(),
            producers: BTreeSet::new(),
            network_mode: "deny".into(),
            domains: BTreeSet::new(),
            manifest_domains: BTreeSet::new(),
            operator_added_domains: BTreeSet::new(),
            operator_denied_domains: BTreeSet::new(),
            tcp: BTreeSet::new(),
            manifest_tcp: BTreeSet::new(),
            operator_added_tcp: BTreeSet::new(),
            operator_denied_tcp: BTreeSet::new(),
            address_cidrs: BTreeSet::new(),
            manifest_cidrs: BTreeSet::new(),
            operator_added_cidrs: BTreeSet::new(),
            allow_undeclared_public_redirects: false,
            operator_allow_undeclared_public_redirects: None,
            bindings: BTreeSet::new(),
            compatibility_flags: BTreeSet::new(),
            cpu_ms: None,
            subrequests: None,
            disk_mib: None,
            memory_mib: None,
            cpu_rate_percent: None,
            extra_processes: None,
            approved_at: String::new(),
        }
    }

    /// Canonical typed network policy for workerd and the native socket proxy.
    #[must_use]
    pub fn egress_policy(&self) -> EgressPolicy {
        let mode = if self.network_mode.eq_ignore_ascii_case("outbound") {
            NetworkMode::Outbound
        } else {
            NetworkMode::Deny
        };
        let mut tcp: Vec<TcpGrant> = self.tcp.iter().cloned().collect();
        tcp.sort();
        EgressPolicy {
            mode,
            domains: self.domains.iter().cloned().collect(),
            max_redirects: bookclerk_plugin_manifest::DEFAULT_MAX_REDIRECTS,
            subrequests: self.subrequests,
            allow_undeclared_public_redirects: self.allow_undeclared_public_redirects,
            tcp,
            address_cidrs: self.address_cidrs.iter().cloned().collect(),
        }
    }
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

    /// Returns the grant whose canonical PluginKey matches `plugin_key`.
    ///
    /// Does not fall back to display aliases — spawn and privilege checks must
    /// not inherit another provenance's grant.
    pub fn get_by_plugin_key(&self, plugin_key: &str) -> Option<&PluginGrant> {
        if plugin_key.is_empty() {
            return None;
        }
        self.grants
            .iter()
            .find(|g| !g.plugin_key.is_empty() && g.plugin_key == plugin_key)
    }

    /// Returns the grant for `plugin_key` (canonical), or an unambiguous alias.
    ///
    /// # Arguments
    ///
    /// * `plugin_key` - Canonical PluginKey text, or a display alias.
    ///
    /// # Returns
    ///
    /// A reference to the matching [`PluginGrant`], or `None`.
    pub fn get(&self, plugin_key: &str) -> Option<&PluginGrant> {
        if let Some(g) = self.get_by_plugin_key(plugin_key) {
            return Some(g);
        }
        let alias_hits: Vec<_> = self
            .grants
            .iter()
            .filter(|g| g.plugin_id == plugin_key)
            .collect();
        match alias_hits.as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    }

    /// Inserts or replaces the grant for `grant.plugin_key` (falling back to alias).
    ///
    /// # Arguments
    ///
    /// * `grant` - Full consent snapshot to persist in memory (call [`Self::save`] to flush).
    pub fn upsert(&mut self, grant: PluginGrant) {
        let idx = self.grants.iter().position(|g| {
            if !grant.plugin_key.is_empty() && !g.plugin_key.is_empty() {
                g.plugin_key == grant.plugin_key
            } else {
                g.plugin_id == grant.plugin_id
            }
        });
        if !grant.plugin_key.is_empty() {
            let revision = crate::authority::authority_revision(&grant);
            crate::authority::fence_stale_sessions(&grant.plugin_key, &revision);
        }
        if let Some(i) = idx {
            self.grants[i] = grant;
        } else {
            self.grants.push(grant);
        }
    }
}

/// Build the consent request surface from a manifest and its PluginKey.
#[must_use]
pub fn consent_request(
    manifest: &PluginManifest,
    plugin_key: &bookclerk_plugin_catalog::PluginKey,
) -> PluginGrant {
    let mut grant = consent_request_alias(manifest);
    grant.plugin_key = plugin_key.canonical().to_string();
    grant
}

/// Build the consent request surface from a manifest (alias only; tests).
#[must_use]
pub fn consent_request_alias(manifest: &PluginManifest) -> PluginGrant {
    let mut bindings = BTreeSet::new();
    let b = manifest.bindings();
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
    for name in &b.databases {
        bindings.insert(format!("{DATABASE_BINDING_PREFIX}{name}"));
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
    let net = &manifest.capabilities.network;
    let tcp: BTreeSet<TcpGrant> = net
        .tcp
        .iter()
        .filter_map(|t| {
            let host = bookclerk_plugin_manifest::normalize_domain_pattern(&t.host)?;
            let mut ports = t.ports.clone();
            ports.sort_unstable();
            ports.dedup();
            Some(TcpGrant { host, ports })
        })
        .collect();
    let address_cidrs: BTreeSet<String> = net
        .address_cidrs
        .iter()
        .filter_map(|raw| {
            bookclerk_plugin_manifest::CidrGrant::parse(raw)
                .ok()
                .map(|g| format!("{}/{}", g.network, g.prefix))
        })
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
        plugin_key: String::new(),
        plugin_id: manifest.id.clone(),
        entrypoints: manifest
            .entrypoints
            .iter()
            .map(|e| e.wire_name().to_string())
            .collect(),
        producers: manifest.producer_types().into_iter().collect(),
        network_mode: match manifest.capabilities.network.mode {
            crate::manifest::NetworkMode::Deny => "deny".into(),
            crate::manifest::NetworkMode::Outbound => "outbound".into(),
        },
        domains: domains.clone(),
        manifest_domains: domains,
        operator_added_domains: BTreeSet::new(),
        operator_denied_domains: BTreeSet::new(),
        tcp: tcp.clone(),
        manifest_tcp: tcp,
        operator_added_tcp: BTreeSet::new(),
        operator_denied_tcp: BTreeSet::new(),
        address_cidrs: address_cidrs.clone(),
        manifest_cidrs: address_cidrs,
        operator_added_cidrs: BTreeSet::new(),
        allow_undeclared_public_redirects: net.allow_undeclared_public_redirects,
        operator_allow_undeclared_public_redirects: None,
        bindings,
        compatibility_flags: flags,
        cpu_ms,
        subrequests,
        disk_mib: Some(PLUGIN_STATE_BUDGET_MIB_DEFAULT),
        memory_mib: Some(PLUGIN_JAIL_MEMORY_MIB_DEFAULT),
        // Native: tunable jail CPU. Workerd: isolate `cpu_ms` + host jail ceiling.
        cpu_rate_percent: if manifest.runtime == PluginRuntimeKind::Workerd {
            None
        } else {
            Some(PLUGIN_JAIL_CPU_RATE_DEFAULT)
        },
        // Native: operator-tunable extra budget. Workerd: host default headroom only.
        extra_processes: if manifest.runtime == PluginRuntimeKind::Workerd {
            None
        } else {
            Some(PLUGIN_JAIL_EXTRA_PROCESSES_DEFAULT)
        },
        approved_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// Human-readable consent lines for CLI/UI.
#[must_use]
pub fn consent_summary(grant: &PluginGrant) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Plugin: {} (entrypoints: {})",
            grant.plugin_id,
            if grant.entrypoints.is_empty() {
                "none".to_string()
            } else {
                grant
                    .entrypoints
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ),
        format!("Network: {}", grant.network_mode),
    ];
    if !grant.producers.is_empty() {
        lines.push(format!(
            "Publishes events: {}",
            grant
                .producers
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if grant.network_mode == "outbound" && grant.domains.is_empty() && grant.tcp.is_empty() {
        lines.push(
            "Outbound mode with no fetch hosts or TCP grants: the guest cannot fetch or \
             connect until the operator adds destinations. Ambient internet sockets are \
             not a grant."
                .into(),
        );
    }
    if !grant.domains.is_empty() {
        lines.push(format!(
            "Workerd initial outbound domains: {}",
            grant.domains.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
        if !grant.operator_added_domains.is_empty() {
            lines.push(format!(
                "Operator-added destinations: {}",
                grant
                    .operator_added_domains
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        lines.push(
            "Redirect destinations must stay within this allowlist unless the operator grants undeclared public redirects; address-space policy still applies."
                .into(),
        );
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
    if !grant.tcp.is_empty() {
        let listed: Vec<String> = grant
            .tcp
            .iter()
            .map(|t| {
                let ports = t
                    .ports
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{}:[{}]", t.host, ports)
            })
            .collect();
        lines.push(format!("Raw TCP connect: {}", listed.join(", ")));
    }
    if !grant.address_cidrs.is_empty() {
        lines.push(format!(
            "Address-space CIDRs (in addition to the public Internet): {}",
            grant
                .address_cidrs
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if grant.allow_undeclared_public_redirects {
        lines.push(
            "Allow redirects to undeclared public destinations (address-space policy still applies)."
                .into(),
        );
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
    let databases = granted_database_bindings(grant);
    if !databases.is_empty() {
        lines.push(format!(
            "Plugin databases (isolated, plugin-owned): {}",
            databases.join(", ")
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
    let disk = effective_disk_mib(grant.disk_mib);
    lines.push(format!(
        "Plugin disk budget: {disk} MiB each for data/ and tmp/ (host max {PLUGIN_STATE_BUDGET_MIB_MAX} MiB)"
    ));
    let memory = effective_memory_mib(grant.memory_mib);
    let proc_line = match grant.extra_processes {
        Some(_) => {
            let extra = effective_extra_processes(grant.extra_processes);
            format!(
                "{extra} additional processes/threads \
                 (host max extra {PLUGIN_JAIL_EXTRA_PROCESSES_MAX})"
            )
        }
        None => "process headroom host-managed (workerd launcher + isolate)".into(),
    };
    let cpu_line = match grant.cpu_rate_percent {
        Some(rate) => format!(
            "{} cores (native jail; host max {})",
            format_cpu_cores(percent_to_cores(effective_cpu_rate_percent(Some(rate)))),
            format_cpu_cores(host_cpu_cores_max())
        ),
        None if grant.cpu_ms.is_some() => format!(
            "CPU rate from host jail settings (workerd isolate budget is cpu_ms; host max {} cores)",
            format_cpu_cores(host_cpu_cores_max())
        ),
        None => format!(
            "{} cores (default; host max {})",
            format_cpu_cores(PLUGIN_JAIL_CPU_CORES_DEFAULT),
            format_cpu_cores(host_cpu_cores_max())
        ),
    };
    lines.push(format!(
        "Jail resources: {memory} MiB memory, {cpu_line}, {proc_line} \
         (host max memory {PLUGIN_JAIL_MEMORY_MIB_MAX} MiB)"
    ));
    lines.push(
        "Operator overrides may widen or narrow the manifest request; host hard caps still \
         apply. Bookclerk does not guarantee plugin behaviour if overrides remove capabilities \
         the guest needs."
            .into(),
    );
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

/// True when an existing grant stays within the manifest request surface.
///
/// Used for **platform auto-grants** (installer envelope). Operator approvals
/// are not limited to this subset — see [`validate_approved_grant`].
#[must_use]
pub fn grant_within_ceiling(existing: &PluginGrant, requested: &PluginGrant) -> bool {
    network_compatible(&existing.network_mode, &requested.network_mode)
        && existing.domains.is_subset(&requested.domains)
        && existing.bindings.is_subset(&requested.bindings)
        && existing
            .compatibility_flags
            .is_subset(&requested.compatibility_flags)
}

/// True when both grants name the same provenance-qualified plugin.
///
/// Empty keys are only equal to other empty keys (test fixtures). A keyed
/// request never inherits an alias-only or differently-keyed grant.
fn plugin_key_matches(existing: &PluginGrant, requested: &PluginGrant) -> bool {
    existing.plugin_key == requested.plugin_key
}

/// True when a stored grant is usable for enable/spawn of this PluginKey.
///
/// Structural capabilities on the current manifest must already be approved
/// (operator may have narrowed, but new entrypoints/bindings require re-consent).
/// Network destinations re-evaluate separately via [`effective_grant`].
#[must_use]
pub fn grant_covers(existing: &PluginGrant, requested: &PluginGrant) -> bool {
    plugin_key_matches(existing, requested)
        && existing.plugin_id == requested.plugin_id
        && requested.entrypoints.is_subset(&existing.entrypoints)
        && requested.producers.is_subset(&existing.producers)
        && requested.bindings.is_subset(&existing.bindings)
        && requested
            .compatibility_flags
            .is_subset(&existing.compatibility_flags)
        && !(requested.allow_undeclared_public_redirects
            && !existing.allow_undeclared_public_redirects)
}

/// Reconstructs operator network additions/denials relative to the current manifest.
fn classified_operator_network(
    existing: &PluginGrant,
    requested: &PluginGrant,
) -> (BTreeSet<String>, BTreeSet<String>) {
    if !existing.operator_added_domains.is_empty() || !existing.operator_denied_domains.is_empty() {
        return (
            existing.operator_added_domains.clone(),
            existing.operator_denied_domains.clone(),
        );
    }
    let manifest = if requested.manifest_domains.is_empty() {
        &requested.domains
    } else {
        &requested.manifest_domains
    };
    let added = existing.domains.difference(manifest).cloned().collect();
    // Legacy grants without explicit denials: do not treat "not yet stored"
    // manifest hosts as denied — those re-evaluate from the new package.
    let denied = existing.operator_denied_domains.clone();
    (added, denied)
}

/// Effective outbound domains: current manifest ∪ operator additions − denials.
fn merge_network_domains(existing: &PluginGrant, requested: &PluginGrant) -> BTreeSet<String> {
    let (added, denied) = classified_operator_network(existing, requested);
    let mut domains = if requested.manifest_domains.is_empty() {
        requested.domains.clone()
    } else {
        requested.manifest_domains.clone()
    };
    domains.extend(added);
    for d in denied {
        domains.remove(&d);
    }
    domains
}

/// Reconstructs operator TCP additions/denials relative to the current manifest.
fn classified_operator_tcp(
    existing: &PluginGrant,
    requested: &PluginGrant,
) -> (BTreeSet<TcpGrant>, BTreeSet<TcpGrant>) {
    if !existing.operator_added_tcp.is_empty() || !existing.operator_denied_tcp.is_empty() {
        return (
            existing.operator_added_tcp.clone(),
            existing.operator_denied_tcp.clone(),
        );
    }
    let added = existing
        .tcp
        .difference(&requested.manifest_tcp)
        .cloned()
        .collect();
    // Legacy grants without explicit TCP denials: new package hosts re-evaluate.
    let denied = existing.operator_denied_tcp.clone();
    (added, denied)
}

/// Operator-added TCP grants relative to the current manifest.
fn merge_operator_tcp(existing: &PluginGrant, requested: &PluginGrant) -> BTreeSet<TcpGrant> {
    classified_operator_tcp(existing, requested).0
}

/// Effective TCP: current manifest ∪ operator additions − operator denials.
fn merge_tcp(existing: &PluginGrant, requested: &PluginGrant) -> BTreeSet<TcpGrant> {
    let (added, denied) = classified_operator_tcp(existing, requested);
    let mut tcp = requested.manifest_tcp.clone();
    tcp.extend(added);
    for grant in denied {
        tcp.remove(&grant);
    }
    tcp
}

/// Effective CIDRs: current manifest ∪ operator additions.
fn merge_cidrs(existing: &PluginGrant, requested: &PluginGrant) -> BTreeSet<String> {
    let added = if existing.operator_added_cidrs.is_empty() {
        existing
            .address_cidrs
            .difference(&requested.manifest_cidrs)
            .cloned()
            .collect()
    } else {
        existing.operator_added_cidrs.clone()
    };
    let mut cidrs = requested.manifest_cidrs.clone();
    cidrs.extend(added);
    cidrs
}

/// Spawn/delivery grant: stored approval is authoritative, host-normalized.
///
/// Structural capabilities stay at the stored operator snapshot (narrowed).
/// Network destinations re-evaluate the current manifest plus persisted
/// operator additions/denials.
#[must_use]
pub fn effective_grant(existing: &PluginGrant, requested: &PluginGrant) -> PluginGrant {
    let network_mode = if existing.network_mode.is_empty() {
        requested.network_mode.clone()
    } else {
        existing.network_mode.clone()
    };
    let (operator_added_domains, operator_denied_domains) =
        classified_operator_network(existing, requested);
    PluginGrant {
        plugin_key: if existing.plugin_key.is_empty() {
            requested.plugin_key.clone()
        } else {
            existing.plugin_key.clone()
        },
        plugin_id: existing.plugin_id.clone(),
        entrypoints: existing
            .entrypoints
            .intersection(&requested.entrypoints)
            .cloned()
            .collect(),
        producers: existing
            .producers
            .intersection(&requested.producers)
            .cloned()
            .collect(),
        network_mode,
        domains: merge_network_domains(existing, requested),
        manifest_domains: if requested.manifest_domains.is_empty() {
            requested.domains.clone()
        } else {
            requested.manifest_domains.clone()
        },
        operator_added_domains,
        operator_denied_domains,
        tcp: merge_tcp(existing, requested),
        manifest_tcp: requested.manifest_tcp.clone(),
        operator_added_tcp: merge_operator_tcp(existing, requested),
        operator_denied_tcp: classified_operator_tcp(existing, requested).1,
        address_cidrs: merge_cidrs(existing, requested),
        manifest_cidrs: requested.manifest_cidrs.clone(),
        operator_added_cidrs: if existing.operator_added_cidrs.is_empty() {
            existing
                .address_cidrs
                .difference(&requested.manifest_cidrs)
                .cloned()
                .collect()
        } else {
            existing.operator_added_cidrs.clone()
        },
        allow_undeclared_public_redirects: existing
            .operator_allow_undeclared_public_redirects
            .unwrap_or(requested.allow_undeclared_public_redirects),
        operator_allow_undeclared_public_redirects: existing
            .operator_allow_undeclared_public_redirects,
        bindings: existing
            .bindings
            .intersection(&requested.bindings)
            .cloned()
            .collect(),
        compatibility_flags: existing
            .compatibility_flags
            .intersection(&requested.compatibility_flags)
            .cloned()
            .collect(),
        cpu_ms: normalize_cpu_ms(existing.cpu_ms.or(requested.cpu_ms)),
        subrequests: normalize_subrequests(existing.subrequests.or(requested.subrequests)),
        disk_mib: Some(effective_disk_mib(existing.disk_mib.or(requested.disk_mib))),
        memory_mib: Some(effective_memory_mib(
            existing.memory_mib.or(requested.memory_mib),
        )),
        cpu_rate_percent: existing
            .cpu_rate_percent
            .or(requested.cpu_rate_percent)
            .map(|v| effective_cpu_rate_percent(Some(v))),
        extra_processes: existing
            .extra_processes
            .or(requested.extra_processes)
            .map(|v| effective_extra_processes(Some(v))),
        approved_at: existing.approved_at.clone(),
    }
}

/// Validates and normalizes an operator-supplied grant against host hard caps.
///
/// The manifest `baseline` supplies identity defaults and suggested values.
/// Operators may **narrow** structural capabilities (entrypoints, producers,
/// host bindings, flags) and **widen or narrow** network destinations.
/// Inventing a structural capability the package did not declare is rejected.
/// Workerd / disk limits clamp to host maximums.
///
/// # Arguments
///
/// * `approved` - Operator-supplied grant draft.
/// * `baseline` - Consent request generated from the current manifest.
///
/// # Returns
///
/// A normalized grant with plugin identity and a fresh `approved_at`.
///
/// # Errors
///
/// Returns [`PluginError`] when identity mismatches, network mode is invalid,
/// a binding is unknown, or a domain pattern cannot be normalized.
///
/// # Panics
///
/// Panics when an internal invariant does not hold.
pub fn validate_approved_grant(
    approved: &PluginGrant,
    baseline: &PluginGrant,
) -> Result<PluginGrant> {
    if !approved.plugin_id.is_empty() && approved.plugin_id != baseline.plugin_id {
        return Err(PluginError::message(format!(
            "grant plugin id `{}` does not match `{}`",
            approved.plugin_id, baseline.plugin_id
        )));
    }
    if let Some(extra) = approved
        .entrypoints
        .difference(&baseline.entrypoints)
        .next()
    {
        return Err(PluginError::message(format!(
            "grant entrypoint `{extra}` is not declared by plugin.toml"
        )));
    }
    if let Some(extra) = approved.producers.difference(&baseline.producers).next() {
        return Err(PluginError::message(format!(
            "grant event producer `{extra}` is not declared by plugin.toml"
        )));
    }
    let network_mode = if approved.network_mode.is_empty() {
        baseline.network_mode.clone()
    } else {
        approved.network_mode.clone()
    };
    if !network_mode.eq_ignore_ascii_case("deny") && !network_mode.eq_ignore_ascii_case("outbound")
    {
        return Err(PluginError::message(format!(
            "invalid network mode `{network_mode}` (expected deny or outbound)"
        )));
    }
    for binding in &approved.bindings {
        if let Some(name) = database_binding_name(binding) {
            if !bookclerk_plugin_manifest::is_valid_database_binding_name(name) {
                return Err(PluginError::message(format!(
                    "invalid database binding name `{name}` (expected [A-Z][A-Z0-9_]*)"
                )));
            }
            if !baseline.bindings.contains(binding) {
                return Err(PluginError::message(format!(
                    "grant database binding `{binding}` is not declared by plugin.toml"
                )));
            }
            continue;
        }
        if !KNOWN_HOST_BINDINGS
            .iter()
            .any(|known| binding.eq_ignore_ascii_case(known))
        {
            return Err(PluginError::message(format!(
                "unknown host binding `{binding}`"
            )));
        }
        let known = KNOWN_HOST_BINDINGS
            .iter()
            .find(|known| binding.eq_ignore_ascii_case(known))
            .copied()
            .unwrap_or(binding.as_str());
        if !baseline
            .bindings
            .iter()
            .any(|b| b.eq_ignore_ascii_case(known) || b == binding)
        {
            return Err(PluginError::message(format!(
                "grant binding `{binding}` is not declared by plugin.toml (structural authority)"
            )));
        }
    }
    if let Some(extra) = approved
        .compatibility_flags
        .difference(&baseline.compatibility_flags)
        .next()
    {
        return Err(PluginError::message(format!(
            "grant compatibility flag `{extra}` is not declared by plugin.toml (structural authority)"
        )));
    }
    let mut domains = BTreeSet::new();
    for raw in &approved.domains {
        let Some(normalized) = bookclerk_plugin_manifest::normalize_domain_pattern(raw) else {
            return Err(PluginError::message(format!(
                "invalid domain pattern `{raw}`"
            )));
        };
        domains.insert(normalized);
    }
    let bindings = approved
        .bindings
        .iter()
        .map(|b| {
            if database_binding_name(b).is_some() {
                // Preserve the case-sensitive binding name after the prefix.
                return b.clone();
            }
            KNOWN_HOST_BINDINGS
                .iter()
                .find(|known| b.eq_ignore_ascii_case(known))
                .map(|known| (*known).to_string())
                .unwrap_or_else(|| b.to_ascii_lowercase())
        })
        .collect();
    let compatibility_flags = approved
        .compatibility_flags
        .iter()
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    let cpu_ms = approved
        .cpu_ms
        .or(baseline.cpu_ms)
        .map(|raw| normalize_cpu_ms(Some(raw)).expect("Some input returns Some"));
    let subrequests = approved
        .subrequests
        .or(baseline.subrequests)
        .map(|raw| normalize_subrequests(Some(raw)).expect("Some input returns Some"));
    let disk_mib = Some(effective_disk_mib(approved.disk_mib.or(baseline.disk_mib)));
    let memory_mib = Some(effective_memory_mib(
        approved.memory_mib.or(baseline.memory_mib),
    ));
    let cpu_rate_percent = approved
        .cpu_rate_percent
        .or(baseline.cpu_rate_percent)
        .map(|v| effective_cpu_rate_percent(Some(v)));
    let extra_processes = approved
        .extra_processes
        .or(baseline.extra_processes)
        .map(|v| effective_extra_processes(Some(v)));
    Ok(PluginGrant {
        plugin_key: baseline.plugin_key.clone(),
        plugin_id: baseline.plugin_id.clone(),
        entrypoints: baseline.entrypoints.clone(),
        producers: baseline.producers.clone(),
        network_mode: network_mode.to_ascii_lowercase(),
        domains: domains.clone(),
        manifest_domains: baseline.domains.clone(),
        operator_added_domains: domains.difference(&baseline.domains).cloned().collect(),
        operator_denied_domains: baseline.domains.difference(&domains).cloned().collect(),
        tcp: approved.tcp.clone(),
        manifest_tcp: baseline.manifest_tcp.clone(),
        operator_added_tcp: approved
            .tcp
            .difference(&baseline.manifest_tcp)
            .cloned()
            .collect(),
        operator_denied_tcp: baseline
            .manifest_tcp
            .difference(&approved.tcp)
            .cloned()
            .collect(),
        address_cidrs: approved.address_cidrs.clone(),
        manifest_cidrs: baseline.manifest_cidrs.clone(),
        operator_added_cidrs: approved
            .address_cidrs
            .difference(&baseline.manifest_cidrs)
            .cloned()
            .collect(),
        allow_undeclared_public_redirects: approved.allow_undeclared_public_redirects,
        operator_allow_undeclared_public_redirects: if approved.allow_undeclared_public_redirects
            != baseline.allow_undeclared_public_redirects
        {
            Some(approved.allow_undeclared_public_redirects)
        } else {
            approved.operator_allow_undeclared_public_redirects
        },
        bindings,
        compatibility_flags,
        cpu_ms,
        subrequests,
        disk_mib,
        memory_mib,
        cpu_rate_percent,
        extra_processes,
        approved_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Clamps an optional CPU budget through [`WorkerdLimits::effective`].
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

/// Clamps an optional subrequest budget through [`WorkerdLimits::effective`].
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

/// Resolved disk budget in MiB (default + host clamp).
#[must_use]
pub fn effective_disk_mib(value: Option<u32>) -> u32 {
    value
        .unwrap_or(PLUGIN_STATE_BUDGET_MIB_DEFAULT)
        .clamp(1, PLUGIN_STATE_BUDGET_MIB_MAX)
}

/// Resolved jail Spec memory ceiling in MiB.
#[must_use]
pub fn effective_memory_mib(value: Option<u32>) -> u32 {
    value
        .unwrap_or(PLUGIN_JAIL_MEMORY_MIB_DEFAULT)
        .clamp(1, PLUGIN_JAIL_MEMORY_MIB_MAX)
}

/// Resolved jail Spec CPU rate as percent of one logical CPU.
///
/// Clamped to `1..=`[`host_cpu_rate_max`] (100 × logical CPUs).
#[must_use]
pub fn effective_cpu_rate_percent(value: Option<u32>) -> u32 {
    value
        .unwrap_or(PLUGIN_JAIL_CPU_RATE_DEFAULT)
        .clamp(1, host_cpu_rate_max())
}

/// Host CPU rate ceiling in one-core percent units (`logical_cpus × 100`).
#[must_use]
pub fn host_cpu_rate_max() -> u32 {
    bookclerk_sandbox::host_cpu_rate_max()
}

/// Logical CPUs visible to this process (at least 1).
#[must_use]
pub fn host_logical_cpus() -> u32 {
    bookclerk_sandbox::host_logical_cpus()
}

/// Format cores for operator-facing text (`0.80`).
#[must_use]
pub fn format_cpu_cores(cores: f64) -> String {
    format!("{:.2}", effective_cpu_cores(Some(cores)))
}

/// Resolved disk budget in bytes for `data/` and `tmp/` checks.
#[must_use]
pub fn effective_disk_budget_bytes(grant: Option<&PluginGrant>) -> u64 {
    u64::from(effective_disk_mib(grant.and_then(|g| g.disk_mib))) * 1024 * 1024
}

/// Require an operator grant before enable **or** every external spawn.
///
/// # Arguments
///
/// * `files_dir` - Bookclerk files directory containing `plugin-grants.json`.
/// * `manifest` - Plugin manifest whose id must have a stored grant.
///
/// # Returns
///
/// Effective [`PluginGrant`] (operator approval, host-normalized).
///
/// # Errors
///
/// Returns [`PluginError`] when no grant exists for the plugin id.
pub fn require_grant(
    files_dir: &Path,
    plugin: &crate::discover::DiscoveredPlugin,
) -> Result<PluginGrant> {
    let store = PluginGrantStore::load(files_dir)?;
    let requested = consent_request(&plugin.manifest, plugin.plugin_key());
    let existing = store.get_by_plugin_key(plugin.plugin_key().canonical());
    match existing {
        Some(existing) if grant_covers(existing, &requested) => {
            Ok(effective_grant(existing, &requested))
        }
        Some(_) => Err(PluginError::message(format!(
            "plugin `{}` grant does not match this plugin; re-approve with `bookclerk plugins approve {}`",
            plugin.alias(), plugin.alias()
        ))),
        None => Err(PluginError::message(format!(
            "plugin `{}` has no permission grant; run `bookclerk plugins approve {}` first",
            plugin.alias(), plugin.alias()
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
///
/// # Errors
///
/// Returns an error when the operation fails.
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

/// Spawn `config` payload: non-empty settings only when the grant includes `config`.
#[must_use]
pub fn spawn_config_for_grant(
    grant: &PluginGrant,
    config_table: serde_json::Value,
) -> serde_json::Value {
    if grant_has_binding(grant, "config") {
        config_table
    } else {
        serde_json::json!({})
    }
}

/// Verified Bookclerk platform artifacts skip the consent UX when the
/// installed payload still matches the host-stamped receipt and the request
/// stays in the installer envelope (deny network, only `config` / `work_fs`).
pub fn ensure_platform_grant(
    files_dir: &Path,
    plugin: &crate::discover::DiscoveredPlugin,
) -> Result<PluginGrant> {
    if !plugin.identity.provenance.grants_platform_defaults()
        || !bookclerk_plugin_catalog::is_platform_plugin_key(plugin.plugin_key())
    {
        return require_grant(files_dir, plugin);
    }
    let requested = consent_request(&plugin.manifest, plugin.plugin_key());
    if !is_safe_platform_request(&requested) {
        return Err(PluginError::message(format!(
            "platform plugin `{}` declares capabilities outside the installer envelope; \
             run `bookclerk plugins approve {}`",
            plugin.alias(),
            plugin.alias()
        )));
    }
    let mut store = PluginGrantStore::load(files_dir)?;
    match store.get_by_plugin_key(plugin.plugin_key().canonical()) {
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

/// Resolve the covering grant for spawn: verified platform auto-grant, else [`require_grant`].
///
/// # Errors
///
/// Returns an error when the operation fails.
pub fn spawn_grant(
    files_dir: &Path,
    plugin: &crate::discover::DiscoveredPlugin,
) -> Result<PluginGrant> {
    if plugin.identity.provenance.grants_platform_defaults()
        && bookclerk_plugin_catalog::is_platform_plugin_key(plugin.plugin_key())
    {
        ensure_platform_grant(files_dir, plugin)
    } else {
        require_grant(files_dir, plugin)
    }
}

/// Injects effective-grant overrides for `bookclerk-workerd` into a guest command.
///
/// Must be called after `env_clear` (explicit `BOOKCLERK_*` is blocked from
/// inheritance). Domains are always set (possibly empty) so a subset approval
/// cannot fall back to the full manifest allowlist.
///
/// # Arguments
///
/// * `cmd` - Guest process command being prepared for spawn.
/// * `grant` - Effective covering grant for this plugin.
pub fn inject_workerd_grant_env(cmd: &mut Command, grant: &PluginGrant) {
    cmd.env(WORKERD_GRANT_NETWORK_MODE_ENV, &grant.network_mode);
    let domains = grant.domains.iter().cloned().collect::<Vec<_>>().join(",");
    cmd.env(WORKERD_GRANT_DOMAINS_ENV, domains);
    if let Some(cpu_ms) = grant.cpu_ms {
        cmd.env(WORKERD_GRANT_CPU_MS_ENV, cpu_ms.to_string());
    }
    if let Some(subrequests) = grant.subrequests {
        cmd.env(WORKERD_GRANT_SUBREQUESTS_ENV, subrequests.to_string());
    }
    if let Ok(policy) = serde_json::to_string(&grant.egress_policy()) {
        cmd.env(WORKERD_GRANT_POLICY_ENV, policy);
    }
}

/// Overlay TCP + CIDR implied by host configuration (not persisted).
///
/// Not operator-invented structural authority. The operator already configured
/// the destination URL; spawn injects matching `EgressPolicy` so the nested
/// native guest can `connect()` it. Covers:
///
/// - active Postgres `[database.postgres].url`
/// - active D1 `[database.d1].api_base`
/// - Audiobookshelf `[integrations.audiobookshelf].base_url`
/// - S3 `[output.s3].endpoint` (custom/MinIO)
///
/// No-op unless the covering grant is outbound.
///
/// # Arguments
///
/// * `grant` - Effective covering grant (mutated in place).
/// * `plugin` - Guest being spawned.
/// * `config` - Host config (URL already env-applied).
pub fn overlay_host_implied_network(
    grant: &mut PluginGrant,
    plugin: &crate::discover::DiscoveredPlugin,
    config: &Config,
) {
    if !grant.network_mode.eq_ignore_ascii_case("outbound") {
        return;
    }
    overlay_postgres_url(grant, plugin, config);
    overlay_d1_api_base(grant, plugin, config);
    overlay_audiobookshelf_url(grant, plugin, config);
    overlay_s3_endpoint(grant, plugin, config);
}

/// Overlay TCP for the active Postgres URL (host-owned, not persisted).
fn overlay_postgres_url(
    grant: &mut PluginGrant,
    plugin: &crate::discover::DiscoveredPlugin,
    config: &Config,
) {
    if DatabasePluginKind::parse(&plugin.manifest.id) != Some(DatabasePluginKind::Postgres) {
        return;
    }
    if DatabasePluginKind::parse(&config.database.plugin) != Some(DatabasePluginKind::Postgres) {
        return;
    }
    let Ok(url) = resolve_postgres_url(config) else {
        return;
    };
    let Some((host, port)) = bookclerk_plugin_database_postgres::postgres_tcp_target(&url) else {
        return;
    };
    overlay_tcp_host(grant, &host, port);
}

/// Overlay TCP for `[database.d1].api_base` when this guest is the active D1 plugin.
fn overlay_d1_api_base(
    grant: &mut PluginGrant,
    plugin: &crate::discover::DiscoveredPlugin,
    config: &Config,
) {
    if DatabasePluginKind::parse(&plugin.manifest.id) != Some(DatabasePluginKind::D1) {
        return;
    }
    if DatabasePluginKind::parse(&config.database.plugin) != Some(DatabasePluginKind::D1) {
        return;
    }
    overlay_http_url(grant, &config.database.d1.api_base, 443);
}

/// Overlay TCP for `[integrations.audiobookshelf].base_url`.
fn overlay_audiobookshelf_url(
    grant: &mut PluginGrant,
    plugin: &crate::discover::DiscoveredPlugin,
    config: &Config,
) {
    if plugin.manifest.id != "audiobookshelf" {
        return;
    }
    overlay_http_url(grant, &config.integrations.audiobookshelf().base_url, 443);
}

/// Overlay TCP for `[output.s3].endpoint` (MinIO / custom). Default AWS hosts
/// come from the S3 guest manifest `tcp` list.
fn overlay_s3_endpoint(
    grant: &mut PluginGrant,
    plugin: &crate::discover::DiscoveredPlugin,
    config: &Config,
) {
    if plugin.manifest.id != "s3" {
        return;
    }
    let Some(endpoint) = config.output.s3.endpoint.as_deref() else {
        return;
    };
    overlay_http_url(grant, endpoint, 443);
}

/// Parses an HTTP(S) URL or bare host and grants TCP + implied CIDRs.
fn overlay_http_url(grant: &mut PluginGrant, raw: &str, default_port: u16) {
    let Some((host, port)) = tcp_target_from_http_url(raw, default_port) else {
        return;
    };
    overlay_tcp_host(grant, &host, port);
}

/// Grants TCP for `host:port` plus loopback/private CIDRs when `host` is not public.
fn overlay_tcp_host(grant: &mut PluginGrant, host: &str, port: u16) {
    add_tcp_grant(grant, host, port);
    for cidr in implied_cidrs_for_host(host) {
        grant.address_cidrs.insert(cidr);
    }
}

/// Host + port for an HTTP(S) URL or a scheme-less host[:port].
fn tcp_target_from_http_url(raw: &str, default_port: u16) -> Option<(String, u16)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = url::Url::parse(trimmed)
        .ok()
        .or_else(|| url::Url::parse(&format!("https://{trimmed}")).ok())?;
    let host = parsed.host_str()?.to_string();
    let port = parsed.port_or_known_default().unwrap_or(default_port);
    Some((host, port))
}

/// Inserts `host:port` into the effective TCP grant set (merging ports).
fn add_tcp_grant(grant: &mut PluginGrant, host: &str, port: u16) {
    let Some(host) = normalize_domain_pattern(host) else {
        return;
    };
    let mut ports = grant
        .tcp
        .iter()
        .find(|t| t.host == host)
        .map(|t| t.ports.clone())
        .unwrap_or_default();
    grant.tcp.retain(|t| t.host != host);
    if !ports.contains(&port) {
        ports.push(port);
        ports.sort_unstable();
    }
    grant.tcp.insert(TcpGrant { host, ports });
}

/// CIDRs the socket proxy needs beyond the public Internet for `host`.
fn implied_cidrs_for_host(host: &str) -> Vec<String> {
    if is_restricted_hostname(host) {
        return vec!["127.0.0.1/32".into(), "::1/128".into()];
    }
    let Ok(ip) = host.parse::<IpAddr>() else {
        return Vec::new();
    };
    if is_public_internet(ip) {
        return Vec::new();
    }
    match ip {
        IpAddr::V4(_) => vec![format!("{ip}/32")],
        IpAddr::V6(_) => vec![format!("{ip}/128")],
    }
}

/// Canonical SHA-256 of a grant's security-relevant fields (not presentation).
#[must_use]
pub fn grant_revision(grant: &PluginGrant) -> String {
    crate::authority::authority_revision(grant)
}

/// True when a platform grant is deny-network with only `config` / `work_fs` bindings.
fn is_safe_platform_request(grant: &PluginGrant) -> bool {
    grant.network_mode == "deny"
        && grant.domains.is_empty()
        && grant.tcp.is_empty()
        && grant.address_cidrs.is_empty()
        && !grant.allow_undeclared_public_redirects
        && grant.compatibility_flags.is_empty()
        && grant
            .bindings
            .iter()
            .all(|b| b == "config" || b == "work_fs")
}

/// Reject `describe()` capability claims that exceed the manifest and the
/// covering grant.
///
/// The guest's typed [`PluginCapabilities`] must not widen what
/// `plugin.toml` declares: no extra entrypoints, event consumers, producers,
/// job types, database bindings, or named bindings. Every advertised
/// entrypoint and producer must also be present in the operator grant, and
/// OAuth storefronts need the `oauth` binding.
///
/// # Arguments
///
/// * `manifest` - Installed manifest for the plugin.
/// * `grant` - Effective covering grant.
/// * `described` - Capability block the guest returned from `describe()`.
/// * `portal_auth_mode` - Storefront connect mode from `describe()`.
///
/// # Errors
///
/// Returns an error naming the first widening capability or missing grant.
pub fn validate_described_capabilities(
    manifest: &PluginManifest,
    grant: &PluginGrant,
    described: &PluginCapabilities,
    portal_auth_mode: PortalAuthMode,
) -> Result<()> {
    let declared = manifest.capabilities();
    let id = &manifest.id;

    for entrypoint in &described.entrypoints {
        if !declared.entrypoints.contains(entrypoint) {
            return Err(PluginError::message(format!(
                "plugin `{id}` describe() exports entrypoint `{}` not declared in plugin.toml",
                entrypoint.wire_name()
            )));
        }
        if !grant.entrypoints.contains(entrypoint.wire_name()) {
            return Err(PluginError::message(format!(
                "plugin `{id}` grant lacks entrypoint `{}`; re-approve with \
                 `bookclerk plugins approve {id}`",
                entrypoint.wire_name()
            )));
        }
    }
    for consumer in &described.consumes {
        let Some(manifest_consumer) = declared
            .consumes
            .iter()
            .find(|c| c.event_type == consumer.event_type)
        else {
            return Err(PluginError::message(format!(
                "plugin `{id}` describe() consumes event `{}` not declared in plugin.toml",
                consumer.event_type
            )));
        };
        if consumer
            .schema_versions
            .iter()
            .any(|v| !manifest_consumer.schema_versions.contains(v))
            || (consumer.supports_suspend && !manifest_consumer.supports_suspend)
        {
            return Err(PluginError::message(format!(
                "plugin `{id}` describe() widens event consumer `{}` beyond plugin.toml",
                consumer.event_type
            )));
        }
    }
    for producer in &described.produces {
        if !declared.produces.contains(producer) {
            return Err(PluginError::message(format!(
                "plugin `{id}` describe() produces event `{producer}` not declared in plugin.toml"
            )));
        }
        if !grant.producers.contains(producer) {
            return Err(PluginError::message(format!(
                "plugin `{id}` grant lacks event producer `{producer}`; re-approve with \
                 `bookclerk plugins approve {id}`"
            )));
        }
    }
    for job in &described.jobs {
        if !declared.jobs.contains(job) {
            return Err(PluginError::message(format!(
                "plugin `{id}` describe() runs job `{job}` not declared in plugin.toml"
            )));
        }
    }
    for database in &described.databases {
        if !declared.databases.contains(database) {
            return Err(PluginError::message(format!(
                "plugin `{id}` describe() binds database `{database}` not declared in plugin.toml"
            )));
        }
        require_binding(grant, &format!("{DATABASE_BINDING_PREFIX}{database}"))?;
    }
    for binding in &described.bindings {
        if !declared.bindings.contains(binding) {
            return Err(PluginError::message(format!(
                "plugin `{id}` describe() expects binding `{binding}` not declared in plugin.toml"
            )));
        }
    }
    if portal_auth_mode == PortalAuthMode::Oauth {
        if !manifest.bindings().oauth {
            return Err(PluginError::message(format!(
                "plugin `{id}` describe() advertises OAuth without `[oauth]` in plugin.toml"
            )));
        }
        require_binding(grant, "oauth")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::DiscoveredPlugin;
    use crate::manifest::PluginManifest;
    use bookclerk_plugin_catalog::PluginProvenance;

    fn discovered(root: &std::path::Path, toml: &str) -> DiscoveredPlugin {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("plugin.toml"), toml).unwrap();
        let command = root.join("guest");
        std::fs::write(&command, b"#!/bin/sh\n").unwrap();
        let manifest = PluginManifest::parse(toml).unwrap();
        DiscoveredPlugin::new(manifest, root.to_path_buf(), command)
    }

    fn stamped_sqlite(root: &std::path::Path) -> DiscoveredPlugin {
        let plugin = discovered(
            root,
            r#"
api_version = 3
id = "sqlite"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "deny"

[vars]

[work_fs]
"#,
        );
        bookclerk_plugin_catalog::stamp_platform_receipt(
            root,
            "bookclerk-plugin-database-sqlite",
            &plugin.manifest,
            "0.0.0",
        )
        .expect("stamp");
        DiscoveredPlugin::new(plugin.manifest, plugin.root, plugin.command)
    }

    fn sample_grant(domains: &[&str], bindings: &[&str], flags: &[&str]) -> PluginGrant {
        PluginGrant {
            plugin_key: String::new(),
            plugin_id: "demo".into(),
            entrypoints: ["storefront".to_string()].into_iter().collect(),
            producers: BTreeSet::new(),
            network_mode: "outbound".into(),
            domains: domains.iter().map(|s| (*s).to_string()).collect(),
            manifest_domains: domains.iter().map(|s| (*s).to_string()).collect(),
            operator_added_domains: BTreeSet::new(),
            operator_denied_domains: BTreeSet::new(),
            bindings: bindings.iter().map(|s| (*s).to_string()).collect(),
            compatibility_flags: flags.iter().map(|s| (*s).to_string()).collect(),
            cpu_ms: None,
            subrequests: None,
            disk_mib: None,
            memory_mib: None,
            cpu_rate_percent: None,
            extra_processes: None,
            approved_at: "2026-01-01T00:00:00Z".into(),
            ..PluginGrant::empty()
        }
    }

    #[test]
    fn grant_covers_allows_operator_domain_subset() {
        let existing = sample_grant(&["a.example"], &["config"], &[]);
        let requested = sample_grant(&["a.example", "b.example"], &["config"], &[]);
        assert!(grant_covers(&existing, &requested));
    }

    #[test]
    fn grant_covers_rejects_new_structural_binding() {
        let existing = sample_grant(&["a.example"], &["config"], &[]);
        let requested = sample_grant(&["a.example"], &["config", "secrets"], &[]);
        assert!(!grant_covers(&existing, &requested));
    }

    #[test]
    fn grant_covers_rejects_new_structural_flag() {
        let existing = sample_grant(&[], &[], &["nodejs_compat"]);
        let requested = sample_grant(&[], &[], &["nodejs_compat", "streams_enable_constructors"]);
        assert!(!grant_covers(&existing, &requested));
    }

    #[test]
    fn grant_covers_allows_stored_capabilities_beyond_manifest_baseline() {
        let existing = sample_grant(
            &["a.example", "b.example"],
            &["config", "secrets"],
            &["nodejs_compat"],
        );
        let requested = sample_grant(&["a.example"], &["config"], &[]);
        assert!(grant_covers(&existing, &requested));
        assert!(!grant_within_ceiling(&existing, &requested));
    }

    #[test]
    fn grant_covers_allows_deny_when_request_is_outbound() {
        let mut existing = sample_grant(&[], &[], &[]);
        existing.network_mode = "deny".into();
        let requested = sample_grant(&[], &[], &[]);
        assert!(grant_covers(&existing, &requested));
    }

    #[test]
    fn overlay_implies_loopback_tcp_from_postgres_url() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            dir.path(),
            r#"
api_version = 3
id = "postgres"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "outbound"

[vars]
"#,
        );
        let mut config = Config::default();
        config.database.plugin = "postgres".into();
        config.database.postgres.url =
            Some("postgres://postgres:postgres@localhost:5432/postgres".into());
        let mut grant = consent_request(&plugin.manifest, plugin.plugin_key());
        overlay_host_implied_network(&mut grant, &plugin, &config);
        let policy = grant.egress_policy();
        assert!(
            policy.allows_tcp("localhost", 5432),
            "implied localhost TCP: {policy:?}"
        );
        assert!(grant.address_cidrs.contains("127.0.0.1/32"));
        assert!(grant.address_cidrs.contains("::1/128"));
    }

    #[test]
    fn overlay_public_host_needs_tcp_not_cidr() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            dir.path(),
            r#"
api_version = 3
id = "postgres"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "outbound"

[vars]
"#,
        );
        let mut config = Config::default();
        config.database.plugin = "postgres".into();
        config.database.postgres.url = Some("postgres://bookclerk@db.example.com:5432/db".into());
        let mut grant = consent_request(&plugin.manifest, plugin.plugin_key());
        overlay_host_implied_network(&mut grant, &plugin, &config);
        assert!(grant.egress_policy().allows_tcp("db.example.com", 5432));
        assert!(grant.address_cidrs.is_empty());
    }

    #[test]
    fn overlay_skips_when_sqlite_is_active() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            dir.path(),
            r#"
api_version = 3
id = "postgres"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "outbound"

[vars]
"#,
        );
        let mut config = Config::default();
        config.database.plugin = "sqlite".into();
        config.database.postgres.url = Some("postgres://localhost/db".into());
        let mut grant = consent_request(&plugin.manifest, plugin.plugin_key());
        overlay_host_implied_network(&mut grant, &plugin, &config);
        assert!(grant.tcp.is_empty());
        assert!(grant.address_cidrs.is_empty());
    }

    #[test]
    fn overlay_d1_api_base_implies_tcp() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            dir.path(),
            r#"
api_version = 3
id = "d1"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "outbound"

[vars]
"#,
        );
        let mut config = Config::default();
        config.database.plugin = "d1".into();
        config.database.d1.api_base = "https://api.cloudflare.com/client/v4".into();
        let mut grant = consent_request(&plugin.manifest, plugin.plugin_key());
        overlay_host_implied_network(&mut grant, &plugin, &config);
        assert!(grant.egress_policy().allows_tcp("api.cloudflare.com", 443));
    }

    #[test]
    fn overlay_audiobookshelf_loopback_implies_cidrs() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            dir.path(),
            r#"
api_version = 3
id = "audiobookshelf"
runtime = "native"
command = "./guest"
entrypoints = ["remoteLibrary"]

[capabilities.network]
mode = "outbound"

[vars]
"#,
        );
        let mut config = Config::default();
        config
            .integrations
            .set_audiobookshelf_string("base_url", "http://127.0.0.1:13378");
        let mut grant = consent_request(&plugin.manifest, plugin.plugin_key());
        overlay_host_implied_network(&mut grant, &plugin, &config);
        assert!(grant.egress_policy().allows_tcp("127.0.0.1", 13378));
        assert!(grant.address_cidrs.contains("127.0.0.1/32"));
    }

    #[test]
    fn overlay_s3_custom_endpoint_implies_tcp() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            dir.path(),
            r#"
api_version = 3
id = "s3"
runtime = "native"
command = "./guest"
entrypoints = ["storage"]

[capabilities.network]
mode = "outbound"

[vars]
"#,
        );
        let mut config = Config::default();
        config.output.s3.endpoint = Some("http://127.0.0.1:9000".into());
        let mut grant = consent_request(&plugin.manifest, plugin.plugin_key());
        overlay_host_implied_network(&mut grant, &plugin, &config);
        assert!(grant.egress_policy().allows_tcp("127.0.0.1", 9000));
        assert!(grant.address_cidrs.contains("127.0.0.1/32"));
    }

    #[test]
    fn grant_revision_changes_with_authority_not_metadata() {
        let a = sample_grant(&["a.example"], &["config"], &[]);
        let mut b = a.clone();
        b.approved_at = "2099-01-01T00:00:00Z".into();
        assert_eq!(grant_revision(&a), grant_revision(&b));
        b.domains.insert("b.example".into());
        assert_ne!(grant_revision(&a), grant_revision(&b));
        assert!(!grant_revision(&a).is_empty());
        assert_eq!(grant_revision(&a).len(), 64);
    }

    #[test]
    fn grant_covers_rejects_mismatched_plugin_keys() {
        let mut a = sample_grant(&["a.example"], &["config"], &[]);
        let mut b = a.clone();
        a.plugin_key = "platform:bookclerk/bookclerk-plugin-database-sqlite#sqlite".into();
        b.plugin_key = "path:file:///tmp/evil#sqlite".into();
        a.plugin_id = "sqlite".into();
        b.plugin_id = "sqlite".into();
        assert!(!grant_covers(&a, &b));
    }

    #[test]
    fn validate_approved_grant_allows_domain_widen() {
        let baseline = sample_grant(&["a.example"], &["config"], &[]);
        let approved = sample_grant(&["a.example", "b.example"], &["config"], &[]);
        let grant = validate_approved_grant(&approved, &baseline).expect("widen ok");
        assert!(grant.domains.contains("a.example"));
        assert!(grant.domains.contains("b.example"));
    }

    #[test]
    fn validate_approved_grant_rejects_unknown_binding() {
        let baseline = sample_grant(&[], &["config"], &[]);
        let approved = sample_grant(&[], &["config", "not_a_real_binding"], &[]);
        let err = validate_approved_grant(&approved, &baseline)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown host binding"), "{err}");
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
    fn consent_request_carries_named_database_bindings() {
        let manifest = PluginManifest::parse(
            r#"
api_version = 3
id = "demo"
runtime = "native"
command = "./demo"
entrypoints = ["cli"]

[capabilities.network]
mode = "deny"

[[databases]]
binding = "DB"

[[databases]]
binding = "CACHE"
"#,
        )
        .expect("manifest");
        let grant = consent_request_alias(&manifest);
        assert!(grant.bindings.contains("database:DB"));
        assert!(grant.bindings.contains("database:CACHE"));
        assert_eq!(granted_database_bindings(&grant), vec!["CACHE", "DB"]);
        let summary = consent_summary(&grant).join("\n");
        assert!(summary.contains("Plugin databases"), "{summary}");
    }

    #[test]
    fn approved_database_bindings_validate_names_and_keep_case() {
        let baseline = sample_grant(&[], &["database:DB_2", "config"], &[]);
        let approved = sample_grant(&[], &["database:DB_2", "config"], &[]);
        let grant = validate_approved_grant(&approved, &baseline).expect("valid binding grant");
        assert!(grant.bindings.contains("database:DB_2"));
        assert_eq!(granted_database_bindings(&grant), vec!["DB_2"]);

        let bad = sample_grant(&[], &["database:not-upper"], &[]);
        let err = validate_approved_grant(&bad, &baseline)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid database binding name"), "{err}");
    }

    #[test]
    fn spawn_config_omitted_without_config_binding() {
        let grant = sample_grant(&[], &["secrets"], &[]);
        let delivered = spawn_config_for_grant(
            &grant,
            serde_json::json!({ "greeting": "hi", "enabled": true }),
        );
        assert_eq!(delivered, serde_json::json!({}));
        let with_config = sample_grant(&[], &["config"], &[]);
        let kept = spawn_config_for_grant(&with_config, serde_json::json!({ "greeting": "hi" }));
        assert_eq!(kept["greeting"], "hi");
    }

    #[test]
    fn require_grant_fails_without_store_entry() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            &dir.path().join("install"),
            r#"
api_version = 3
id = "demo"
runtime = "native"
command = "./guest"
entrypoints = ["storefront"]

[capabilities.network]
mode = "deny"

[vars]
"#,
        );
        let err = require_grant(dir.path(), &plugin).unwrap_err().to_string();
        assert!(err.contains("no permission grant"), "{err}");
    }

    #[test]
    fn require_grant_requires_reapprove_for_new_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            &dir.path().join("install"),
            r#"
api_version = 3
id = "demo"
runtime = "native"
command = "./guest"
entrypoints = ["storefront"]

[capabilities.network]
mode = "deny"

[vars]

[secrets]
"#,
        );
        let mut store = PluginGrantStore::default();
        let mut existing = sample_grant(&[], &["config"], &[]);
        existing.network_mode = "deny".into();
        existing.plugin_key = plugin.plugin_key().canonical().to_string();
        store.upsert(existing);
        store.save(dir.path()).unwrap();

        let err = require_grant(dir.path(), &plugin).unwrap_err().to_string();
        assert!(
            err.contains("grant does not match") || err.contains("re-approve"),
            "{err}"
        );
    }

    #[test]
    fn require_grant_keeps_operator_domain_widen_past_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            &dir.path().join("install"),
            r#"
api_version = 3
id = "demo"
runtime = "workerd"
entrypoints = ["storefront"]

[workerd]
compatibility_date = "2026-08-01"
main_module = "index.js"

[capabilities.network]
mode = "outbound"
domains = ["api.example.com"]

[vars]
"#,
        );
        let mut store = PluginGrantStore::default();
        let mut existing = sample_grant(&["old.example"], &["config"], &[]);
        existing.approved_at = "2026-01-01T00:00:00Z".into();
        existing.plugin_key = plugin.plugin_key().canonical().to_string();
        store.upsert(existing);
        store.save(dir.path()).unwrap();

        let grant = require_grant(dir.path(), &plugin).expect("operator widen kept");
        assert!(grant.domains.contains("old.example"));
        assert!(
            grant.domains.contains("api.example.com"),
            "new manifest hosts re-evaluate on the same PluginKey"
        );
        assert!(grant.operator_added_domains.contains("old.example"));
    }

    #[test]
    fn platform_grant_auto_persists_for_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = stamped_sqlite(&dir.path().join("install"));
        assert_eq!(
            plugin.identity.provenance,
            PluginProvenance::PlatformBundled
        );
        let grant = ensure_platform_grant(dir.path(), &plugin).unwrap();
        assert!(grant_has_binding(&grant, "config"));
        assert!(grant_has_binding(&grant, "work_fs"));
        assert!(!grant.plugin_key.is_empty());
        let again = spawn_grant(dir.path(), &plugin).unwrap();
        assert_eq!(again.plugin_id, "sqlite");
        assert_eq!(again.approved_at, grant.approved_at);
        assert_eq!(again.bindings, grant.bindings);
        assert_eq!(again.plugin_key, plugin.plugin_key().canonical());
    }

    #[test]
    fn fake_sqlite_id_does_not_receive_platform_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            &dir.path().join("install"),
            r#"
api_version = 3
id = "sqlite"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "deny"

[vars]

[work_fs]
"#,
        );
        assert_eq!(
            plugin.identity.provenance,
            PluginProvenance::LocalDevelopment
        );
        assert!(!plugin.identity.provenance.grants_platform_defaults());
        let err = ensure_platform_grant(dir.path(), &plugin)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no permission grant"), "{err}");
    }

    #[test]
    fn platform_grant_replaces_stale_grant_when_manifest_narrows() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            &dir.path().join("install"),
            r#"
api_version = 3
id = "sqlite"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "deny"

[vars]
"#,
        );
        bookclerk_plugin_catalog::stamp_platform_receipt(
            &plugin.root,
            "bookclerk-plugin-database-sqlite",
            &plugin.manifest,
            "0.0.0",
        )
        .unwrap();
        let plugin = DiscoveredPlugin::new(plugin.manifest, plugin.root, plugin.command);

        let mut store = PluginGrantStore::default();
        let mut existing = sample_grant(&[], &["config", "work_fs"], &[]);
        existing.plugin_key = plugin.plugin_key().canonical().to_string();
        existing.plugin_id = "sqlite".into();
        existing.entrypoints = ["databaseAdapter".to_string()].into_iter().collect();
        existing.network_mode = "deny".into();
        existing.approved_at = "2026-02-02T00:00:00Z".into();
        store.upsert(existing);
        store.save(dir.path()).unwrap();

        let grant = ensure_platform_grant(dir.path(), &plugin).unwrap();
        assert_eq!(
            grant.bindings.iter().cloned().collect::<Vec<_>>(),
            vec!["config".to_string()]
        );
        assert!(!grant_has_binding(&grant, "work_fs"));
    }

    #[test]
    fn platform_grant_fails_outside_installer_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = discovered(
            &dir.path().join("install"),
            r#"
api_version = 3
id = "sqlite"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "deny"

[vars]

[secrets]
"#,
        );
        bookclerk_plugin_catalog::stamp_platform_receipt(
            &plugin.root,
            "bookclerk-plugin-database-sqlite",
            &plugin.manifest,
            "0.0.0",
        )
        .unwrap();
        let plugin = DiscoveredPlugin::new(plugin.manifest, plugin.root, plugin.command);
        let err = ensure_platform_grant(dir.path(), &plugin)
            .unwrap_err()
            .to_string();
        assert!(err.contains("outside the installer envelope"), "{err}");
    }

    #[test]
    fn effective_grant_intersects_structural_and_keeps_operator_network() {
        let existing = sample_grant(
            &["a.example", "b.example"],
            &["config", "secrets"],
            &["nodejs_compat"],
        );
        let requested = sample_grant(&["a.example"], &["config"], &[]);
        let effective = effective_grant(&existing, &requested);
        assert_eq!(effective.plugin_id, existing.plugin_id);
        assert_eq!(effective.entrypoints, existing.entrypoints);
        assert_eq!(effective.approved_at, existing.approved_at);
        assert_eq!(effective.network_mode, existing.network_mode);
        assert!(effective.domains.contains("a.example"));
        assert!(effective.domains.contains("b.example"));
        assert!(effective.operator_added_domains.contains("b.example"));
        assert_eq!(
            effective.bindings.iter().cloned().collect::<Vec<_>>(),
            vec!["config".to_string()]
        );
        assert!(effective.compatibility_flags.is_empty());
        assert_eq!(effective.disk_mib, Some(PLUGIN_STATE_BUDGET_MIB_DEFAULT));
    }

    #[test]
    fn effective_grant_keeps_operator_tcp_denials_across_manifest_upgrade() {
        let denied = TcpGrant {
            host: "old.example.com".into(),
            ports: vec![443],
        };
        let added = TcpGrant {
            host: "extra.example.com".into(),
            ports: vec![443],
        };
        let mut existing = sample_grant(&["api.example.com"], &["config"], &[]);
        existing.tcp.insert(added.clone());
        existing.operator_added_tcp.insert(added.clone());
        existing.operator_denied_tcp.insert(denied.clone());
        existing.manifest_tcp.insert(denied.clone());

        let mut requested = sample_grant(&["api.example.com"], &["config"], &[]);
        requested.tcp.insert(denied.clone());
        requested.manifest_tcp.insert(denied.clone());
        requested.tcp.insert(TcpGrant {
            host: "new.example.com".into(),
            ports: vec![443],
        });
        requested.manifest_tcp.insert(TcpGrant {
            host: "new.example.com".into(),
            ports: vec![443],
        });

        let effective = effective_grant(&existing, &requested);
        assert!(
            !effective.tcp.contains(&denied),
            "operator TCP denials must survive a same-PluginKey package upgrade"
        );
        assert!(effective.tcp.contains(&added));
        assert!(effective.tcp.iter().any(|t| t.host == "new.example.com"));
        assert!(effective.operator_denied_tcp.contains(&denied));
    }

    #[test]
    fn require_grant_does_not_inherit_across_plugin_keys() {
        let dir = tempfile::tempdir().unwrap();
        let platformish = discovered(
            &dir.path().join("a"),
            r#"
api_version = 3
id = "sqlite"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "deny"
"#,
        );
        let impostor = discovered(
            &dir.path().join("b"),
            r#"
api_version = 3
id = "sqlite"
runtime = "native"
command = "./guest"
entrypoints = ["databaseAdapter"]

[capabilities.network]
mode = "deny"
"#,
        );
        assert_ne!(
            platformish.plugin_key().canonical(),
            impostor.plugin_key().canonical()
        );
        let mut store = PluginGrantStore::default();
        let mut existing = consent_request(&platformish.manifest, platformish.plugin_key());
        existing.approved_at = "2026-01-01T00:00:00Z".into();
        store.upsert(existing);
        store.save(dir.path()).unwrap();
        let err = require_grant(dir.path(), &impostor)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no permission grant"), "{err}");
    }

    #[test]
    fn upsert_fences_live_session_for_plugin_key() {
        let key = "path:file:///tmp/demo#demo";
        let flag = crate::authority::register_session(key, "old-revision");
        let mut store = PluginGrantStore::default();
        let mut grant = sample_grant(&["a.example"], &["config"], &[]);
        grant.plugin_key = key.into();
        store.upsert(grant);
        assert!(crate::authority::is_fenced(&flag));
        crate::authority::unregister_session(&flag);
    }

    #[test]
    fn require_grant_keeps_stored_subset_after_repeated_manifest_widening() {
        let dir = tempfile::tempdir().unwrap();
        let narrowed = discovered(
            &dir.path().join("install"),
            r#"
api_version = 3
id = "demo"
runtime = "native"
command = "./guest"
entrypoints = ["storefront"]

[capabilities.network]
mode = "deny"

[vars]
"#,
        );
        let mut store = PluginGrantStore::default();
        let mut existing = sample_grant(&[], &["config"], &[]);
        existing.network_mode = "deny".into();
        existing.plugin_key = narrowed.plugin_key().canonical().to_string();
        store.upsert(existing);
        store.save(dir.path()).unwrap();

        let effective = require_grant(dir.path(), &narrowed).unwrap();
        assert!(grant_has_binding(&effective, "config"));

        let widened = discovered(
            &dir.path().join("install"),
            r#"
api_version = 3
id = "demo"
runtime = "native"
command = "./guest"
entrypoints = ["storefront"]

[capabilities.network]
mode = "deny"

[vars]

[[kv_namespaces]]
binding = "KV"
"#,
        );
        let err = require_grant(dir.path(), &widened).unwrap_err().to_string();
        assert!(
            err.contains("grant does not match") || err.contains("re-approve"),
            "new structural KV binding must require re-consent: {err}"
        );
    }

    #[test]
    fn validate_approved_grant_allows_network_widen_not_structural() {
        let baseline = sample_grant(&["api.example.com"], &["config"], &["nodejs_compat"]);
        let approved = sample_grant(
            &["api.example.com", "extra.example.com"],
            &["config"],
            &["nodejs_compat"],
        );
        let grant = validate_approved_grant(&approved, &baseline).expect("network widen ok");
        assert!(grant.domains.contains("extra.example.com"));
        assert!(grant.operator_added_domains.contains("extra.example.com"));
        assert_eq!(grant.manifest_domains, baseline.domains);

        let steal = sample_grant(
            &["api.example.com"],
            &["config", "secrets"],
            &["nodejs_compat"],
        );
        let err = validate_approved_grant(&steal, &baseline)
            .unwrap_err()
            .to_string();
        assert!(err.contains("structural"), "{err}");
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
        assert_eq!(normalized.entrypoints, ceiling.entrypoints);
        assert_eq!(normalized.cpu_ms, Some(15_000));
        assert_eq!(normalized.subrequests, Some(25));
        assert!(grant_has_binding(&normalized, "config"));
        assert!(!grant_has_binding(&normalized, "secrets"));
        assert_ne!(normalized.approved_at, approved.approved_at);
    }

    #[test]
    fn validate_approved_grant_clamps_limits_to_host_max_not_baseline() {
        let mut baseline = sample_grant(&[], &[], &[]);
        baseline.cpu_ms = Some(30_000);
        baseline.subrequests = Some(50);
        let mut approved = baseline.clone();
        approved.cpu_ms = Some(60_000);
        approved.subrequests = Some(200);
        approved.disk_mib = Some(PLUGIN_STATE_BUDGET_MIB_MAX + 50);
        let grant = validate_approved_grant(&approved, &baseline).expect("host clamp");
        assert_eq!(grant.cpu_ms, Some(60_000));
        assert_eq!(grant.subrequests, Some(200));
        assert_eq!(grant.disk_mib, Some(PLUGIN_STATE_BUDGET_MIB_MAX));
    }

    fn caps(entrypoints: &[bookclerk_plugin_abi::Entrypoint]) -> PluginCapabilities {
        PluginCapabilities {
            entrypoints: entrypoints.to_vec(),
            ..PluginCapabilities::default()
        }
    }

    #[test]
    fn validate_describe_rejects_oauth_without_binding() {
        let manifest = PluginManifest::parse(
            r#"
api_version = 3
id = "demo"
runtime = "native"
command = "./demo"
entrypoints = ["storefront"]

[capabilities.network]
mode = "deny"

[vars]
"#,
        )
        .unwrap();
        let grant = consent_request_alias(&manifest);
        let err = validate_described_capabilities(
            &manifest,
            &grant,
            &caps(&[bookclerk_plugin_abi::Entrypoint::Storefront]),
            PortalAuthMode::Oauth,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("OAuth"), "{err}");
    }

    #[test]
    fn validate_describe_rejects_undeclared_entrypoints_and_events() {
        let manifest = PluginManifest::parse(
            r#"
api_version = 3
id = "demo"
runtime = "native"
command = "./demo"
entrypoints = ["cli"]

[capabilities.network]
mode = "deny"

[[events.consumers]]
type = "book_acquired"
schema_versions = [1]
"#,
        )
        .unwrap();
        let grant = consent_request_alias(&manifest);
        let err = validate_described_capabilities(
            &manifest,
            &grant,
            &caps(&[
                bookclerk_plugin_abi::Entrypoint::Cli,
                bookclerk_plugin_abi::Entrypoint::RemoteLibrary,
            ]),
            PortalAuthMode::Unspecified,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("remoteLibrary"), "{err}");

        let widened_consumer = PluginCapabilities {
            consumes: vec![bookclerk_plugin_abi::EventConsumerSpec {
                event_type: "book_acquired".into(),
                schema_versions: vec![1, 2],
                supports_suspend: false,
            }],
            ..caps(&[bookclerk_plugin_abi::Entrypoint::Cli])
        };
        let err = validate_described_capabilities(
            &manifest,
            &grant,
            &widened_consumer,
            PortalAuthMode::Unspecified,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("widens event consumer"), "{err}");

        let undeclared_producer = PluginCapabilities {
            produces: vec!["book_acquired".into()],
            ..caps(&[bookclerk_plugin_abi::Entrypoint::Cli])
        };
        let err = validate_described_capabilities(
            &manifest,
            &grant,
            &undeclared_producer,
            PortalAuthMode::Unspecified,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("produces event"), "{err}");
    }

    #[test]
    fn validate_describe_accepts_manifest_capabilities() {
        let manifest = PluginManifest::parse(
            r#"
api_version = 3
id = "demo"
runtime = "native"
command = "./demo"
entrypoints = ["cli", "remoteLibrary"]

[capabilities.network]
mode = "deny"

[vars]

[[databases]]
binding = "DB"

[[events.consumers]]
type = "book_acquired"
schema_versions = [1]
supports_suspend = true

[[events.producers]]
type = "demo_pinged"
"#,
        )
        .unwrap();
        let grant = consent_request_alias(&manifest);
        assert!(grant.producers.contains("demo_pinged"));
        assert!(grant.entrypoints.contains("remoteLibrary"));
        validate_described_capabilities(
            &manifest,
            &grant,
            &manifest.capabilities(),
            PortalAuthMode::Unspecified,
        )
        .unwrap();

        // A grant that lost the producer no longer covers the manifest request.
        let mut narrowed = grant.clone();
        narrowed.producers.clear();
        assert!(!grant_covers(&narrowed, &grant));
        let err = validate_described_capabilities(
            &manifest,
            &narrowed,
            &manifest.capabilities(),
            PortalAuthMode::Unspecified,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("grant lacks event producer"), "{err}");
    }

    #[test]
    fn python_outbound_consent_includes_pyodide_hosts() {
        let manifest = PluginManifest::parse(
            r#"
api_version = 3
id = "echo_workerd_python"
runtime = "workerd"
entrypoints = ["cli"]

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

[vars]
"#,
        )
        .unwrap();
        let grant = consent_request_alias(&manifest);
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
        // Narrow stored grant for the same plugin id remains usable.
        let mut narrow = sample_grant(&["api.example.com"], &["config"], &["python_workers"]);
        narrow.plugin_id = grant.plugin_id.clone();
        narrow.entrypoints = grant.entrypoints.clone();
        assert!(grant_covers(&narrow, &grant));
        assert!(grant_within_ceiling(&narrow, &grant));
    }

    #[test]
    fn workerd_grant_env_keys_match_launcher_contract() {
        // Keep in lockstep with bookclerk_workerd::grant constants (stringly
        // coupled across crates; no shared dep either direction).
        assert_eq!(
            WORKERD_GRANT_NETWORK_MODE_ENV,
            "BOOKCLERK_WORKERD_GRANT_NETWORK_MODE"
        );
        assert_eq!(WORKERD_GRANT_DOMAINS_ENV, "BOOKCLERK_WORKERD_GRANT_DOMAINS");
        assert_eq!(WORKERD_GRANT_CPU_MS_ENV, "BOOKCLERK_WORKERD_GRANT_CPU_MS");
        assert_eq!(
            WORKERD_GRANT_SUBREQUESTS_ENV,
            "BOOKCLERK_WORKERD_GRANT_SUBREQUESTS"
        );
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
