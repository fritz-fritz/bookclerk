//! Host egress policy for workerd guests.

use bookclerk_plugin_host::{NetworkMode, PluginManifest};

/// Initial-host allowlist with redirect-follow semantics.
#[derive(Debug, Clone)]
pub struct EgressProxy {
    mode: NetworkMode,
    domains: Vec<String>,
    max_redirects: u32,
}

impl EgressProxy {
    /// Network mode from the plugin manifest.
    #[must_use]
    pub fn mode(&self) -> NetworkMode {
        self.mode
    }

    /// Maximum redirect hops after an allowed initial request.
    #[must_use]
    pub fn max_redirects(&self) -> u32 {
        self.max_redirects
    }
}

impl EgressProxy {
    #[must_use]
    pub fn from_manifest(manifest: &PluginManifest) -> Self {
        Self {
            mode: manifest.capabilities.network.mode,
            domains: manifest.capabilities.network.domains.clone(),
            max_redirects: 10,
        }
    }

    #[must_use]
    pub fn allowed_initial_hosts(&self) -> &[String] {
        &self.domains
    }

    /// Whether a *direct* (non-redirect) request host is permitted.
    #[must_use]
    pub fn allows_initial_host(&self, host: &str) -> bool {
        match self.mode {
            NetworkMode::Deny => false,
            NetworkMode::Outbound => self.domains.iter().any(|d| host_matches(host, d)),
        }
    }

    /// Redirect hops are allowed without re-checking the domain allowlist.
    #[must_use]
    pub fn allows_redirect_hop(&self, _host: &str, hop_index: u32) -> bool {
        match self.mode {
            NetworkMode::Deny => false,
            NetworkMode::Outbound => hop_index < self.max_redirects,
        }
    }
}

fn host_matches(host: &str, pattern: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let pattern = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(rest) = pattern.strip_prefix("*.") {
        host == rest || host.ends_with(&format!(".{rest}"))
    } else {
        host == pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_and_redirect_policy() {
        let proxy = EgressProxy {
            mode: NetworkMode::Outbound,
            domains: vec!["api.example.com".into(), "*.cdn.example.com".into()],
            max_redirects: 5,
        };
        assert!(proxy.allows_initial_host("api.example.com"));
        assert!(proxy.allows_initial_host("a.cdn.example.com"));
        assert!(!proxy.allows_initial_host("evil.com"));
        assert!(proxy.allows_redirect_hop("evil.com", 1));
        assert!(!proxy.allows_redirect_hop("evil.com", 5));
    }
}
