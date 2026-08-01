//! Per-operation filesystem grants for Windows guests (AppContainer ACL).
//!
//! Unix guests receive open directory/file descriptors over `SCM_RIGHTS` on
//! fd 3. Windows has no equivalent side channel once the process is running in
//! an AppContainer; instead the host temporarily ACLs the target path for the
//! guest's Package SID, passes the path on the JSON-RPC wire, then removes the
//! ACE after the call returns.
//!
//! This module is the planned API surface. Full `SetNamedSecurityInfo` wiring
//! lands with AppContainer `CreateProcess` in `bookclerk-jail`.

#![cfg(windows)]

use std::path::Path;

use crate::{PluginError, Result};

/// Grant the AppContainer Package SID access to `path` for the duration of one
/// RPC (fetch dir / upload file / sqlite db).
///
/// # Errors
///
/// Currently always returns a clear "not yet enabled" error so callers fail
/// closed under `plugins.isolation = required` rather than silently widening
/// access.
pub fn grant_path_for_guest(_package_sid: &str, _path: &Path, _write: bool) -> Result<AclGuard> {
    Err(PluginError::message(
        "Windows per-operation ACL grants require AppContainer CreateProcess \
         (see bookclerk_sandbox::spawn); use isolation=best-effort for path-based \
         unconfined guests during the transition",
    ))
}

/// RAII guard that revokes the temporary ACE on drop.
pub struct AclGuard {
    // Reserved for SID + path when CreateProcess AppContainer lands.
}

impl Drop for AclGuard {
    fn drop(&mut self) {
        // Revoke ACE when implemented.
    }
}
