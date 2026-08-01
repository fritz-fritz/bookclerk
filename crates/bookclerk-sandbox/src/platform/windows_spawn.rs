//! Windows AppContainer spawn scaffolding.
//!
//! Windows cannot confine a process after it has started. Isolation is granted
//! at `CreateProcess` by giving the child a token with an AppContainer SID and
//! named capability SIDs, then ACLing the paths it may reach.
//!
//! This module plans those capabilities from a [`Policy`]. The actual
//! `CreateProcess` AppContainer path is **not enabled** in this build —
//! [`spawn_appcontainer`] returns a clear error so callers can fall back to
//! `plugins.isolation = "best-effort"` (unconfined) until CreateProcess wiring
//! lands.

use std::ffi::OsString;
use std::path::Path;
use std::process::Child;

use crate::{NetPolicy, Policy, SandboxError};

/// Planned AppContainer launch parameters (not yet applied at CreateProcess).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppContainerLaunch {
    /// Placeholder for the AppContainer package SID once profiles are created.
    pub package_sid: Option<String>,
    /// Named capability SIDs to grant (well-known capability names).
    pub capability_names: Vec<&'static str>,
}

/// Map a confinement [`Policy`] into AppContainer network capabilities.
///
/// | [`NetPolicy`] | Capabilities |
/// | --- | --- |
/// | [`Deny`](NetPolicy::Deny) | _(none)_ |
/// | [`Outbound`](NetPolicy::Outbound) | `internetClient` |
/// | [`OutboundListen`](NetPolicy::OutboundListen) | `internetClient`, `privateNetworkClientServer` |
/// | [`Full`](NetPolicy::Full) | `internetClient`, `internetClientServer`, `privateNetworkClientServer` |
///
/// Filesystem ACL planning for `resolved_reads` / `resolved_writes` is left to
/// the future CreateProcess path.
pub fn plan_appcontainer(policy: &Policy) -> Result<AppContainerLaunch, SandboxError> {
    let capability_names = match policy.net_policy() {
        NetPolicy::Deny => Vec::new(),
        NetPolicy::Outbound => vec!["internetClient"],
        NetPolicy::OutboundListen => vec!["internetClient", "privateNetworkClientServer"],
        NetPolicy::Full => vec![
            "internetClient",
            "internetClientServer",
            "privateNetworkClientServer",
        ],
    };
    Ok(AppContainerLaunch {
        package_sid: None,
        capability_names,
    })
}

/// Spawn `program` inside an AppContainer derived from `policy`.
///
/// # Errors
///
/// Always returns [`SandboxError::Backend`] in this build — CreateProcess with
/// an AppContainer token is not wired yet. Operators should set
/// `plugins.isolation = "best-effort"` (or `off`) on Windows until it is.
pub fn spawn_appcontainer(
    policy: &Policy,
    _program: &Path,
    _args: &[OsString],
) -> Result<Child, SandboxError> {
    // Keep planning so call sites exercise the same path once CreateProcess lands.
    let _plan = plan_appcontainer(policy)?;
    Err(SandboxError::Backend {
        label: policy.label().to_string(),
        backend: "appcontainer",
        detail: "AppContainer CreateProcess not yet enabled in this build; \
                 set plugins.isolation=best-effort"
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Policy;

    #[test]
    fn deny_maps_to_no_network_caps() {
        let plan = plan_appcontainer(&Policy::new("t").net(NetPolicy::Deny)).unwrap();
        assert!(plan.capability_names.is_empty());
        assert!(plan.package_sid.is_none());
    }

    #[test]
    fn outbound_maps_to_internet_client() {
        let plan = plan_appcontainer(&Policy::new("t").net(NetPolicy::Outbound)).unwrap();
        assert_eq!(plan.capability_names, ["internetClient"]);
    }

    #[test]
    fn outbound_listen_adds_private_network() {
        let plan = plan_appcontainer(&Policy::new("t").net(NetPolicy::OutboundListen)).unwrap();
        assert_eq!(
            plan.capability_names,
            ["internetClient", "privateNetworkClientServer"]
        );
    }
}
