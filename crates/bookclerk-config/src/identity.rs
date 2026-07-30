//! Dedicated OS service identity for Bookclerk.
//!
//! Production `bookclerkd` must not run as the interactive login user. Prefer a
//! system account (`bookclerk` by default). When started as root, Bookclerk can
//! drop privileges to that account before opening `master.key` / the library DB.
//!
//! Local development under `$HOME` or `/tmp` is allowed unless the operator
//! forces a hard requirement. Override with `BOOKCLERK_ALLOW_USER_RUN=1`.

#![allow(unsafe_code)] // setuid/getpwnam / GetUserNameW

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Default system account for the daemon / files-dir ownership.
pub const DEFAULT_SERVICE_USER: &str = "bookclerk";
/// Default primary group for the service account.
pub const DEFAULT_SERVICE_GROUP: &str = "bookclerk";

/// `[daemon.identity]` — who Bookclerk runs as.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct IdentityConfig {
    /// Required OS account name for production daemon runs.
    pub service_user: String,
    /// Primary group for [`Self::service_user`] (Unix).
    pub service_group: String,
    /// When true and started as root/Administrator, drop to [`Self::service_user`]
    /// before touching secrets.
    pub drop_privileges: bool,
    /// Allow running as an interactive login user (desktop / `cargo run` under
    /// `$HOME`). Production files dirs (`/var/lib/bookclerk`, …) still refuse
    /// unless this is true or `BOOKCLERK_ALLOW_USER_RUN` is set.
    pub allow_interactive_user: bool,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            service_user: DEFAULT_SERVICE_USER.into(),
            service_group: DEFAULT_SERVICE_GROUP.into(),
            drop_privileges: true,
            allow_interactive_user: false,
        }
    }
}

/// Outcome of [`apply_daemon_identity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityStatus {
    /// OS user name after enforcement / drop.
    pub user: String,
    /// Numeric user id when available (Unix); `None` on Windows name-only checks.
    pub uid: Option<u32>,
    /// Whether privileges were dropped from root/Administrator.
    pub dropped: bool,
}

/// Apply daemon identity policy for `files_dir`.
///
/// Call once early in `bookclerkd` (after config load, before master-key / DB).
pub fn apply_daemon_identity(
    identity: &IdentityConfig,
    files_dir: &Path,
) -> Result<IdentityStatus> {
    if allow_user_run_env() {
        return Ok(current_status(false));
    }

    platform::apply(identity, files_dir)
}

/// True when the operator explicitly permits interactive-user runs.
#[must_use]
pub fn allow_user_run_env() -> bool {
    match std::env::var("BOOKCLERK_ALLOW_USER_RUN") {
        Ok(v) => {
            let v = v.trim();
            v == "1"
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
                || v.eq_ignore_ascii_case("on")
        }
        Err(_) => false,
    }
}

/// Heuristic: files dir looks like a personal / scratch tree (dev), not a
/// system service data root.
#[must_use]
pub fn looks_like_dev_files_dir(files_dir: &Path) -> bool {
    let s = files_dir.to_string_lossy();
    if s.contains("/tmp/")
        || s.starts_with("/tmp")
        || s.contains("\\Temp\\")
        || s.contains("\\tmp\\")
    {
        return true;
    }
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        if !home.is_empty() && s.starts_with(&home) {
            return true;
        }
    }
    // Workspace / cargo run convenience.
    if s.contains("BookclerkFiles") || s.contains("bookclerk-files") {
        return true;
    }
    false
}

fn current_status(dropped: bool) -> IdentityStatus {
    platform::current_status(dropped)
}

#[cfg(unix)]
mod platform {
    use super::{looks_like_dev_files_dir, IdentityConfig, IdentityStatus, DEFAULT_SERVICE_USER};
    use crate::error::{ConfigError, Result};
    use std::ffi::CStr;
    use std::path::Path;

    pub(super) fn current_status(dropped: bool) -> IdentityStatus {
        let uid = unsafe { libc::geteuid() };
        let user = username_for_uid(uid).unwrap_or_else(|| format!("uid:{uid}"));
        IdentityStatus {
            user,
            uid: Some(uid),
            dropped,
        }
    }

    pub(super) fn apply(identity: &IdentityConfig, files_dir: &Path) -> Result<IdentityStatus> {
        let euid = unsafe { libc::geteuid() };
        let allow_dev = identity.allow_interactive_user || looks_like_dev_files_dir(files_dir);

        if euid == 0 && identity.drop_privileges {
            match drop_to(identity) {
                Ok(()) => return Ok(current_status(true)),
                Err(err) if allow_dev => {
                    tracing::warn!(
                        %err,
                        files_dir = %files_dir.display(),
                        "root privilege drop skipped (service user missing); continuing in interactive/dev mode"
                    );
                    return Ok(current_status(false));
                }
                Err(err) => return Err(err),
            }
        }

        match lookup_user(&identity.service_user) {
            Ok(expected) if euid == expected.uid => Ok(current_status(false)),
            Ok(expected) if allow_dev => {
                tracing::warn!(
                    current_uid = euid,
                    expected_user = %identity.service_user,
                    expected_uid = expected.uid,
                    files_dir = %files_dir.display(),
                    "running bookclerkd as interactive/dev user; production should use the `{DEFAULT_SERVICE_USER}` system account"
                );
                Ok(current_status(false))
            }
            Ok(expected) => Err(ConfigError::Invalid(format!(
                "bookclerkd must run as system user `{}` (uid {}), not uid {}. \
                 Install the systemd/launchd unit, or for local dev set \
                 daemon.identity.allow_interactive_user=true / BOOKCLERK_ALLOW_USER_RUN=1",
                identity.service_user, expected.uid, euid
            ))),
            Err(err) if allow_dev => {
                tracing::warn!(
                    %err,
                    files_dir = %files_dir.display(),
                    "service user `{}` unavailable; continuing in interactive/dev mode",
                    identity.service_user
                );
                Ok(current_status(false))
            }
            Err(err) => Err(err),
        }
    }

    struct Account {
        uid: u32,
        gid: u32,
    }

    fn lookup_user(name: &str) -> Result<Account> {
        let cname = std::ffi::CString::new(name).map_err(|_| {
            ConfigError::Invalid(format!("service user name `{name}` contains NUL"))
        })?;
        // SAFETY: getpwnam returns a static pw pointer; we copy uid/gid immediately.
        let pw = unsafe { libc::getpwnam(cname.as_ptr()) };
        if pw.is_null() {
            return Err(ConfigError::Invalid(format!(
                "service user `{name}` does not exist — create it (useradd --system {name}) \
                 or set daemon.identity.allow_interactive_user=true for local development"
            )));
        }
        let uid = unsafe { (*pw).pw_uid };
        let gid = unsafe { (*pw).pw_gid };
        Ok(Account { uid, gid })
    }

    fn lookup_group(name: &str) -> Result<u32> {
        let cname = std::ffi::CString::new(name).map_err(|_| {
            ConfigError::Invalid(format!("service group name `{name}` contains NUL"))
        })?;
        let gr = unsafe { libc::getgrnam(cname.as_ptr()) };
        if gr.is_null() {
            return Err(ConfigError::Invalid(format!(
                "service group `{name}` does not exist"
            )));
        }
        Ok(unsafe { (*gr).gr_gid })
    }

    fn drop_to(identity: &IdentityConfig) -> Result<()> {
        let account = lookup_user(&identity.service_user)?;
        let gid = lookup_group(&identity.service_group).unwrap_or(account.gid);
        let cname = std::ffi::CString::new(identity.service_user.as_str())
            .map_err(|_| ConfigError::Invalid("service user name contains NUL".into()))?;

        // SAFETY: setgid/initgroups/setuid are the standard privilege-drop sequence.
        #[allow(unsafe_code)]
        unsafe {
            if libc::setgid(gid) != 0 {
                return Err(ConfigError::Io(std::io::Error::last_os_error()));
            }
            if libc::initgroups(cname.as_ptr(), gid) != 0 {
                return Err(ConfigError::Io(std::io::Error::last_os_error()));
            }
            if libc::setuid(account.uid) != 0 {
                return Err(ConfigError::Io(std::io::Error::last_os_error()));
            }
        }
        tracing::info!(
            user = %identity.service_user,
            uid = account.uid,
            gid,
            "dropped privileges to service account"
        );
        Ok(())
    }

    fn username_for_uid(uid: u32) -> Option<String> {
        // SAFETY: getpwuid returns static data; we copy the name out.
        let pw = unsafe { libc::getpwuid(uid) };
        if pw.is_null() {
            return None;
        }
        let name = unsafe { CStr::from_ptr((*pw).pw_name) };
        Some(name.to_string_lossy().into_owned())
    }
}

#[cfg(windows)]
mod platform {
    use super::{looks_like_dev_files_dir, IdentityConfig, IdentityStatus, DEFAULT_SERVICE_USER};
    use crate::error::{ConfigError, Result};
    use std::path::Path;

    pub(super) fn current_status(dropped: bool) -> IdentityStatus {
        IdentityStatus {
            user: current_username().unwrap_or_else(|| "unknown".into()),
            uid: None,
            dropped,
        }
    }

    pub(super) fn apply(identity: &IdentityConfig, files_dir: &Path) -> Result<IdentityStatus> {
        let current = current_username().unwrap_or_default();
        let expected = identity.service_user.as_str();

        if current.eq_ignore_ascii_case(expected)
            || current.eq_ignore_ascii_case(&format!(".\\{expected}"))
            || current
                .to_ascii_lowercase()
                .ends_with(&format!("\\{}", expected.to_ascii_lowercase()))
        {
            return Ok(current_status(false));
        }

        // SYSTEM / LocalSystem are acceptable for Windows service hosts that
        // then impersonate; treat as needing drop guidance rather than OK.
        let is_system = current.eq_ignore_ascii_case("SYSTEM")
            || current.eq_ignore_ascii_case("NT AUTHORITY\\SYSTEM")
            || current.eq_ignore_ascii_case("LocalSystem");

        if is_system && identity.drop_privileges {
            return Err(ConfigError::Invalid(format!(
                "bookclerkd is running as {current}; configure the Windows service to Log On As \
                 local user `{expected}` (see packaging/windows/)"
            )));
        }

        if identity.allow_interactive_user || looks_like_dev_files_dir(files_dir) {
            tracing::warn!(
                %current,
                expected_user = %expected,
                files_dir = %files_dir.display(),
                "running bookclerkd as interactive/dev user; production should use the `{DEFAULT_SERVICE_USER}` account"
            );
            return Ok(current_status(false));
        }

        Err(ConfigError::Invalid(format!(
            "bookclerkd must run as Windows account `{expected}`, not `{current}`. \
             Configure the service Log On As that account, or for local dev set \
             daemon.identity.allow_interactive_user=true / BOOKCLERK_ALLOW_USER_RUN=1"
        )))
    }

    fn current_username() -> Option<String> {
        use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;

        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        // SAFETY: GetUserNameW writes into buf; len includes NUL on input/output.
        #[allow(unsafe_code)]
        let ok = unsafe { GetUserNameW(buf.as_mut_ptr(), &mut len) };
        if ok == 0 || len == 0 {
            return None;
        }
        let end = (len as usize).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_files_dir_heuristic() {
        assert!(looks_like_dev_files_dir(Path::new("/tmp/BookclerkFiles")));
        assert!(looks_like_dev_files_dir(Path::new(
            "/home/alice/BookclerkFiles"
        )));
        assert!(!looks_like_dev_files_dir(Path::new("/var/lib/bookclerk")));
    }

    #[test]
    fn default_identity_targets_bookclerk() {
        let id = IdentityConfig::default();
        assert_eq!(id.service_user, "bookclerk");
        assert!(id.drop_privileges);
        assert!(!id.allow_interactive_user);
    }

    #[test]
    fn apply_allows_dev_tree_without_service_user() {
        // Even when service user is missing on this CI image, /tmp is allowed.
        let id = IdentityConfig {
            service_user: "bookclerk-does-not-exist-zz".into(),
            allow_interactive_user: false,
            ..IdentityConfig::default()
        };
        let status = apply_daemon_identity(&id, Path::new("/tmp/BookclerkFiles-test")).unwrap();
        assert!(!status.user.is_empty());
    }
}
