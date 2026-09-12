//! Canonical effective authority and live session fencing.
//!
//! Structural capabilities originate in the plugin package. The operator may
//! narrow them but cannot invent entrypoints, producers, or host bindings.
//! Network destinations are operator-extensible: explicit host-owned grants
//! may add hostnames beyond the manifest. Guest `describe()` cannot widen
//! either class.
//!
//! ```text
//! effective structural = manifest ∩ operator grant ∩ host policy
//! effective network    = host policy ∩ approved(manifest network ∪ operator additions)
//!                      − operator denials
//! ```
//!
//! `authority_revision` is SHA-256 of the canonical effective policy. Grant
//! changes fence already-running sessions that still hold the old revision.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use sha2::{Digest, Sha256};

use crate::consent::PluginGrant;
use crate::PluginError;

/// Live plugin sessions keyed by provenance-qualified PluginKey.
struct LiveSession {
    /// Canonical PluginKey for this vat.
    plugin_key: String,
    /// [`authority_revision`] captured at spawn.
    revision: String,
    /// Set when a later grant for this key no longer matches `revision`.
    cancelled: Arc<AtomicBool>,
}

/// Process-wide live session table.
fn live() -> &'static Mutex<Vec<LiveSession>> {
    static LIVE: OnceLock<Mutex<Vec<LiveSession>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Registers a live session and returns its cancel flag.
///
/// Drop the session (or call [`unregister_session`]) when the vat exits.
#[must_use]
pub fn register_session(plugin_key: &str, revision: &str) -> Arc<AtomicBool> {
    let cancelled = Arc::new(AtomicBool::new(false));
    if let Ok(mut guard) = live().lock() {
        guard.push(LiveSession {
            plugin_key: plugin_key.to_string(),
            revision: revision.to_string(),
            cancelled: Arc::clone(&cancelled),
        });
    }
    cancelled
}

/// Drops a previously registered cancel flag from the live set.
pub fn unregister_session(flag: &Arc<AtomicBool>) {
    if let Ok(mut guard) = live().lock() {
        guard.retain(|s| !Arc::ptr_eq(&s.cancelled, flag));
    }
}

/// Fences every live session for `plugin_key` (grant/authority change).
pub fn fence_plugin_key(plugin_key: &str) {
    fence_stale_sessions(plugin_key, "");
}

/// Fences live sessions for `plugin_key` whose revision is not `current_revision`.
///
/// Pass an empty `current_revision` to fence every session for the key.
pub fn fence_stale_sessions(plugin_key: &str, current_revision: &str) {
    if plugin_key.is_empty() {
        return;
    }
    if let Ok(guard) = live().lock() {
        for session in guard.iter() {
            if session.plugin_key == plugin_key
                && (current_revision.is_empty() || session.revision != current_revision)
            {
                session.cancelled.store(true, Ordering::SeqCst);
            }
        }
    }
}

/// True when this cancel flag has been fenced.
#[must_use]
pub fn is_fenced(flag: &Arc<AtomicBool>) -> bool {
    flag.load(Ordering::SeqCst)
}

/// Error when a session's authority revision is no longer current.
pub fn fenced_error() -> PluginError {
    PluginError::message("plugin session fenced: effective authority changed")
}

/// Canonical SHA-256 of security-relevant effective authority (not presentation).
///
/// Includes structural capabilities, network mode, approved domains, operator
/// additions/denials, and bindings. Omits `approved_at` and display aliases
/// except the PluginKey.
#[must_use]
pub fn authority_revision(grant: &PluginGrant) -> String {
    let mut hasher = Sha256::new();
    hasher.update(grant.plugin_key.as_bytes());
    hasher.update(b"\nstructural\n");
    for e in &grant.entrypoints {
        hasher.update(e.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\nproducers\n");
    for p in &grant.producers {
        hasher.update(p.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\nbindings\n");
    for b in &grant.bindings {
        hasher.update(b.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\nflags\n");
    for f in &grant.compatibility_flags {
        hasher.update(f.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\nnetwork\n");
    hasher.update(grant.network_mode.as_bytes());
    hasher.update(b"\ndomains\n");
    for d in &grant.domains {
        hasher.update(d.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\nmanifest_domains\n");
    for d in &grant.manifest_domains {
        hasher.update(d.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\noperator_added\n");
    for d in &grant.operator_added_domains {
        hasher.update(d.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\noperator_denied\n");
    for d in &grant.operator_denied_domains {
        hasher.update(d.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\ntcp\n");
    hash_tcp_grants(&mut hasher, &grant.tcp);
    hasher.update(b"\noperator_added_tcp\n");
    hash_tcp_grants(&mut hasher, &grant.operator_added_tcp);
    hasher.update(b"\noperator_denied_tcp\n");
    hash_tcp_grants(&mut hasher, &grant.operator_denied_tcp);
    hasher.update(b"\ncidrs\n");
    for c in &grant.address_cidrs {
        hasher.update(c.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\noperator_added_cidrs\n");
    for c in &grant.operator_added_cidrs {
        hasher.update(c.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\noperator_denied_cidrs\n");
    for c in &grant.operator_denied_cidrs {
        hasher.update(c.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\nredirects\n");
    hasher.update(
        u8::from(grant.allow_undeclared_public_redirects)
            .to_string()
            .as_bytes(),
    );
    hex::encode(hasher.finalize())
}

/// Canonical digest of TCP grants (`host:port,…;`).
fn hash_tcp_grants(hasher: &mut Sha256, grants: &BTreeSet<bookclerk_plugin_manifest::TcpGrant>) {
    for t in grants {
        hasher.update(t.host.as_bytes());
        hasher.update(b":");
        for p in &t.ports {
            hasher.update(p.to_string().as_bytes());
            hasher.update(b",");
        }
        hasher.update(b";");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn grant(domains: &[&str]) -> PluginGrant {
        PluginGrant {
            plugin_key: "path:file:///tmp/demo#demo".into(),
            plugin_id: "demo".into(),
            entrypoints: ["storefront".to_string()].into_iter().collect(),
            producers: BTreeSet::new(),
            network_mode: "outbound".into(),
            domains: domains.iter().map(|s| (*s).to_string()).collect(),
            manifest_domains: domains.iter().map(|s| (*s).to_string()).collect(),
            operator_added_domains: BTreeSet::new(),
            operator_denied_domains: BTreeSet::new(),
            bindings: ["config".to_string()].into_iter().collect(),
            compatibility_flags: BTreeSet::new(),
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
    fn metadata_does_not_change_authority_revision() {
        let a = grant(&["a.example"]);
        let mut b = a.clone();
        b.approved_at = "2099-01-01T00:00:00Z".into();
        b.plugin_id = "other-alias".into();
        assert_eq!(authority_revision(&a), authority_revision(&b));
    }

    #[test]
    fn operator_added_domain_changes_revision() {
        let a = grant(&["a.example"]);
        let mut b = a.clone();
        b.domains.insert("b.example".into());
        b.operator_added_domains.insert("b.example".into());
        assert_ne!(authority_revision(&a), authority_revision(&b));
    }

    #[test]
    fn fence_stale_sessions_keeps_current_revision() {
        let key = "path:file:///tmp/keep#demo";
        let current = register_session(key, "rev-new");
        let stale = register_session(key, "rev-old");
        fence_stale_sessions(key, "rev-new");
        assert!(!is_fenced(&current));
        assert!(is_fenced(&stale));
        unregister_session(&current);
        unregister_session(&stale);
    }

    #[test]
    fn tcp_and_cidr_changes_revision() {
        let a = grant(&["a.example"]);
        let mut b = a.clone();
        b.tcp.insert(bookclerk_plugin_manifest::TcpGrant {
            host: "db.example.com".into(),
            ports: vec![5432],
        });
        assert_ne!(authority_revision(&a), authority_revision(&b));
        let mut c = a.clone();
        c.address_cidrs.insert("10.0.60.100/32".into());
        assert_ne!(authority_revision(&a), authority_revision(&c));
        let mut d = a.clone();
        d.allow_undeclared_public_redirects = true;
        assert_ne!(authority_revision(&a), authority_revision(&d));
        let mut e = a.clone();
        e.operator_denied_tcp
            .insert(bookclerk_plugin_manifest::TcpGrant {
                host: "cdn.example.com".into(),
                ports: vec![443],
            });
        assert_ne!(authority_revision(&a), authority_revision(&e));
        let mut f = a.clone();
        f.operator_denied_cidrs.insert("10.0.0.0/8".into());
        assert_ne!(authority_revision(&a), authority_revision(&f));
    }
}
