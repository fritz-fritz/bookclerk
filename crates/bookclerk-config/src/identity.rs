//! Dedicated OS service identity for Bookclerk.
//!
//! Production `bookclerkd` runs as the system account `bookclerk` (not the
//! interactive login user). Typical install:
//!
//! 1. A **user-level** unit / tray is owned by the installing user (session UI).
//! 2. The daemon starts privileged enough to drop (root, or setuid-root helper)
//!    and **drops to `bookclerk`** before opening `master.key` / the library DB
//!    (Linux: `setuid` + retained `CAP_CHOWN`; macOS: `seteuid` so real uid
//!    stays 0 for later chown; Windows: Log On As the service account).
//! 3. The installing user’s name is captured into `BOOKCLERK_OUTPUT_OWNER` (when
//!    unset) so `@user/Audiobooks` resolves under their home with their uid/gid.
//!
//! Fail-closed: when `drop_privileges` is set and the process is root, a failed
//! drop refuses to continue (unless `BOOKCLERK_ALLOW_USER_RUN=1`).

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
    /// When true and effective uid is root, drop to [`Self::service_user`]
    /// before touching secrets. Fail-closed if the drop cannot complete.
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
///
/// When `capture_output_owner` is true and `BOOKCLERK_OUTPUT_OWNER` is unset,
/// records the installing / real user into that env var so `@user/Audiobooks`
/// ownership survives the privilege drop. Pass `false` when
/// `output.local.owner_user` is already set in config so a late capture cannot
/// override an explicit TOML owner (env set by the operator/unit still wins in
/// [`crate::resolve_local_file_owner`]).
pub fn apply_daemon_identity(
    identity: &IdentityConfig,
    files_dir: &Path,
    capture_output_owner: bool,
) -> Result<IdentityStatus> {
    if capture_output_owner {
        capture_output_owner_env();
    }

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
/// production service data root.
///
/// Matches only well-scoped layouts (exact `/tmp` prefix, a `BookclerkFiles`
/// path component, or under `$HOME` / `%USERPROFILE%` **except** the
/// production user-install path `~/.local/share/bookclerk`). That XDG path is
/// what the Linux user-unit install script uses — it must still require the
/// setuid-root helper → drop to `bookclerk`, not silently run as the login user.
#[must_use]
pub fn looks_like_dev_files_dir(files_dir: &Path) -> bool {
    use std::path::Component;

    let s = files_dir.to_string_lossy();
    if s == "/tmp"
        || s.starts_with("/tmp/")
        || s.contains("\\Temp\\")
        || s.contains("\\tmp\\")
        || s.ends_with("\\Temp")
        || s.ends_with("\\tmp")
    {
        return true;
    }
    // Workspace / cargo run convenience — path *component*, not substring.
    if files_dir.components().any(|c| match c {
        Component::Normal(name) => name == "BookclerkFiles" || name == "bookclerk-files",
        _ => false,
    }) {
        return true;
    }
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        if !home.is_empty() {
            let home = Path::new(&home);
            if files_dir.starts_with(home) && !is_xdg_user_install_files_dir(files_dir, home) {
                return true;
            }
        }
    }
    false
}

/// `~/.local/share/bookclerk` — production user-unit files dir (not "dev").
fn is_xdg_user_install_files_dir(files_dir: &Path, home: &Path) -> bool {
    let Ok(rel) = files_dir.strip_prefix(home) else {
        return false;
    };
    rel == Path::new(".local/share/bookclerk")
        || rel.starts_with(Path::new(".local/share/bookclerk"))
}

/// Record the installing / real user for `@user/Audiobooks` when unset.
///
/// Only fills `BOOKCLERK_OUTPUT_OWNER` when the operator has not already set
/// it. Callers that have an explicit `output.local.owner_user` should skip
/// this so a late capture cannot invent an env value that overrides TOML
/// (see [`crate::resolve_local_file_owner`] — env wins over config).
pub fn capture_output_owner_env() {
    if std::env::var_os("BOOKCLERK_OUTPUT_OWNER").is_some_and(|v| !v.is_empty()) {
        return;
    }
    if let Some(name) = platform::installing_username() {
        // SAFETY: set_var is process-wide; called once at daemon/CLI startup
        // before worker threads touch output paths.
        unsafe { std::env::set_var("BOOKCLERK_OUTPUT_OWNER", &name) };
        tracing::debug!(owner = %name, "captured BOOKCLERK_OUTPUT_OWNER before identity drop");
    }
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

    /// Real (pre-setuid) user when available; otherwise interactive non-service name.
    pub(super) fn installing_username() -> Option<String> {
        if let Ok(v) = std::env::var("SUDO_USER") {
            let t = v.trim();
            if !t.is_empty() && t != "root" && t != DEFAULT_SERVICE_USER {
                return Some(t.to_string());
            }
        }
        let ruid = unsafe { libc::getuid() };
        let euid = unsafe { libc::geteuid() };
        // setuid-root helper: real user is the installing user.
        if euid == 0 && ruid != 0 {
            if let Some(name) = username_for_uid(ruid) {
                if name != "root" && name != DEFAULT_SERVICE_USER {
                    return Some(name);
                }
            }
        }
        if euid != 0 {
            if let Some(name) = username_for_uid(euid) {
                if name != "root" && name != DEFAULT_SERVICE_USER {
                    return Some(name);
                }
            }
        }
        None
    }

    pub(super) fn apply(identity: &IdentityConfig, files_dir: &Path) -> Result<IdentityStatus> {
        let euid = unsafe { libc::geteuid() };
        let allow_dev = identity.allow_interactive_user || looks_like_dev_files_dir(files_dir);

        if euid == 0 && identity.drop_privileges {
            // Fail-closed: never continue as root when a drop was requested.
            drop_to(identity)?;
            return Ok(current_status(true));
        }

        if euid == 0 && !identity.drop_privileges {
            return Err(ConfigError::Invalid(
                "bookclerkd is running as root with daemon.identity.drop_privileges=false; \
                 refuse to keep root. Enable drop_privileges or set BOOKCLERK_ALLOW_USER_RUN=1"
                    .into(),
            ));
        }

        match lookup_user(&identity.service_user) {
            Ok(expected) if euid == expected.uid => Ok(current_status(false)),
            Ok(expected) if allow_dev => {
                tracing::warn!(
                    current_uid = euid,
                    expected_user = %identity.service_user,
                    expected_uid = expected.uid,
                    files_dir = %files_dir.display(),
                    "running bookclerkd as interactive/dev user; production should drop to `{DEFAULT_SERVICE_USER}`"
                );
                Ok(current_status(false))
            }
            Ok(expected) => Err(ConfigError::Invalid(format!(
                "bookclerkd must run as system user `{}` (uid {}), not uid {}. \
                 Use the user/system unit (drops to bookclerk), or for local dev set \
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
            // Linux: keep capabilities across setuid so we can retain CAP_CHOWN
            // for `@user/Audiobooks` ownership after running as bookclerk.
            #[cfg(target_os = "linux")]
            if libc::prctl(libc::PR_SET_KEEPCAPS, 1, 0, 0, 0) != 0 {
                return Err(ConfigError::Io(std::io::Error::last_os_error()));
            }

            if libc::setgid(gid) != 0 {
                return Err(ConfigError::Io(std::io::Error::last_os_error()));
            }
            // Apple's initgroups takes `c_int` for basegroup; Linux takes `gid_t`.
            #[cfg(target_os = "macos")]
            let init_gid = i32::try_from(gid).map_err(|_| {
                ConfigError::Invalid(format!("service gid {gid} does not fit initgroups"))
            })?;
            #[cfg(not(target_os = "macos"))]
            let init_gid = gid;
            if libc::initgroups(cname.as_ptr(), init_gid) != 0 {
                return Err(ConfigError::Io(std::io::Error::last_os_error()));
            }
            // macOS has no CAP_CHOWN: keep real uid 0 and only drop the
            // *effective* uid so local acquire can briefly `seteuid(0)` to
            // chown media to the installing user. Linux uses full setuid +
            // retained CAP_CHOWN instead.
            #[cfg(target_os = "macos")]
            if libc::seteuid(account.uid) != 0 {
                return Err(ConfigError::Io(std::io::Error::last_os_error()));
            }
            #[cfg(not(target_os = "macos"))]
            if libc::setuid(account.uid) != 0 {
                return Err(ConfigError::Io(std::io::Error::last_os_error()));
            }
        }

        #[cfg(target_os = "linux")]
        retain_cap_chown();

        tracing::info!(
            user = %identity.service_user,
            uid = account.uid,
            gid,
            "dropped privileges to service account"
        );
        Ok(())
    }

    /// After setuid, keep only `CAP_CHOWN` so local acquire can chown media to
    /// the installing user. Failure is non-fatal (chown becomes best-effort).
    #[cfg(target_os = "linux")]
    fn retain_cap_chown() {
        use caps::{CapSet, Capability, CapsHashSet};

        let mut wanted = CapsHashSet::new();
        wanted.insert(Capability::CAP_CHOWN);
        if let Err(err) = caps::set(None, CapSet::Permitted, &wanted) {
            tracing::warn!(%err, "could not restrict permitted caps to CAP_CHOWN after drop");
            return;
        }
        if let Err(err) = caps::set(None, CapSet::Effective, &wanted) {
            tracing::warn!(%err, "could not raise CAP_CHOWN after privilege drop; local chown may EPERM");
            return;
        }
        let _ = caps::clear(None, CapSet::Inheritable);
        tracing::debug!("retained CAP_CHOWN after privilege drop for local audiobook ownership");
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

    pub(super) fn installing_username() -> Option<String> {
        current_username().and_then(|name| {
            let lower = name.to_ascii_lowercase();
            if lower == "system"
                || lower.ends_with("\\system")
                || lower == DEFAULT_SERVICE_USER
                || lower.ends_with(&format!("\\{}", DEFAULT_SERVICE_USER))
            {
                None
            } else {
                Some(name)
            }
        })
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
        // Must not match arbitrary substrings.
        assert!(!looks_like_dev_files_dir(Path::new("/var/tmpfoo/data")));
        assert!(!looks_like_dev_files_dir(Path::new(
            "/data/notBookclerkFiles-extra"
        )));
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
        let id = IdentityConfig {
            service_user: "bookclerk-does-not-exist-zz".into(),
            allow_interactive_user: false,
            ..IdentityConfig::default()
        };
        let files = Path::new("/tmp/BookclerkFiles-test");

        #[cfg(unix)]
        if unsafe { libc::geteuid() } == 0 {
            // Fail-closed: root must not continue when the service user is missing.
            let err = apply_daemon_identity(&id, files, true).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("does not exist") || msg.contains("drop"),
                "unexpected error: {msg}"
            );
            return;
        }

        // Non-root: /tmp is treated as a dev tree and may continue with a warning.
        let status = apply_daemon_identity(&id, files, true).unwrap();
        assert!(!status.user.is_empty());
    }

    #[test]
    fn xdg_user_install_is_not_dev_heuristic() {
        // Production user-unit files dir must not auto-allow interactive runs.
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/alice".into());
        let xdg = Path::new(&home).join(".local/share/bookclerk");
        assert!(
            !looks_like_dev_files_dir(&xdg),
            "XDG user install should require bookclerk identity / setuid helper"
        );
        // Scratch trees under $HOME remain interactive-dev.
        let scratch = Path::new(&home).join("BookclerkFiles");
        assert!(looks_like_dev_files_dir(&scratch));
    }
}
