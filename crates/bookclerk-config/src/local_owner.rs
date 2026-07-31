//! Resolve who should own local acquired audiobooks (uid/gid / Windows account + home).

#![allow(unsafe_code)] // getpwnam / getpwuid / LookupAccountNameW

use std::path::{Path, PathBuf};

use crate::output::{OutputLocalConfig, OUTPUT_LOCAL_USER_ROOT};

/// Resolved OS identity for `[output.local]` ownership and `@user/…` roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFileOwner {
    /// Account name (or decimal uid string when looked up by number).
    pub user: String,
    /// Group name (or decimal gid), when resolved.
    pub group: Option<String>,
    pub home: PathBuf,
    #[cfg(unix)]
    pub uid: u32,
    #[cfg(unix)]
    pub gid: u32,
}

/// Resolve the file owner for local output.
///
/// Order (env wins over config, standard Bookclerk practice):
/// `BOOKCLERK_OUTPUT_OWNER` → `output.local.owner_user` → `SUDO_USER` →
/// current interactive user (skips `root` / `bookclerk`).
///
/// Group: `BOOKCLERK_OUTPUT_OWNER_GROUP` → `output.local.owner_group` →
/// owner's primary group (Unix).
///
/// `owner_user` / `owner_group` accept an account **name** or a decimal
/// **id** (Unix uid/gid; Windows accepts names or `S-1-…` SID strings).
#[must_use]
pub fn resolve_local_file_owner(local: &OutputLocalConfig) -> Option<LocalFileOwner> {
    let name = env_nonempty("BOOKCLERK_OUTPUT_OWNER")
        .or_else(|| {
            local
                .owner_user
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .or_else(|| env_nonempty("SUDO_USER"))
        .or_else(interactive_username)?;

    let group = env_nonempty("BOOKCLERK_OUTPUT_OWNER_GROUP").or_else(|| {
        local
            .owner_group
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    });

    platform::lookup_owner(&name, group.as_deref())
}

/// Expand `@user` / `@user/Audiobooks` using `owner`, or return `None` if the
/// path is not a user-root sentinel.
#[must_use]
pub fn expand_user_local_root(root: &Path, owner: &LocalFileOwner) -> Option<PathBuf> {
    let raw = root.to_string_lossy();
    if raw == OUTPUT_LOCAL_USER_ROOT || raw == format!("{OUTPUT_LOCAL_USER_ROOT}/") {
        return Some(owner.home.join("Audiobooks"));
    }
    if let Some(rest) = raw.strip_prefix(&format!("{OUTPUT_LOCAL_USER_ROOT}/")) {
        if rest.is_empty() {
            return Some(owner.home.join("Audiobooks"));
        }
        return Some(owner.home.join(rest));
    }
    None
}

/// True when `root` still needs `@user` expansion.
#[must_use]
pub fn is_user_local_root(root: &Path) -> bool {
    let raw = root.to_string_lossy();
    raw == OUTPUT_LOCAL_USER_ROOT
        || raw == format!("{OUTPUT_LOCAL_USER_ROOT}/")
        || raw.starts_with(&format!("{OUTPUT_LOCAL_USER_ROOT}/"))
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn interactive_username() -> Option<String> {
    platform::current_username().and_then(|name| {
        let lower = name.to_ascii_lowercase();
        if lower == "root" || lower == "bookclerk" || lower == "system" {
            None
        } else {
            Some(name)
        }
    })
}

#[cfg(unix)]
mod platform {
    use super::LocalFileOwner;
    use std::ffi::CStr;
    use std::path::PathBuf;

    pub(super) fn current_username() -> Option<String> {
        let uid = unsafe { libc::geteuid() };
        username_for_uid(uid)
    }

    pub(super) fn lookup_owner(user: &str, group: Option<&str>) -> Option<LocalFileOwner> {
        let (uid, user_name, home, default_gid) = lookup_user(user)?;
        let (gid, group_name) = match group {
            Some(g) => lookup_group(g).map(|(gid, name)| (gid, Some(name)))?,
            None => (default_gid, group_name_for_gid(default_gid)),
        };
        Some(LocalFileOwner {
            user: user_name,
            group: group_name,
            home,
            uid,
            gid,
        })
    }

    fn lookup_user(user: &str) -> Option<(u32, String, PathBuf, u32)> {
        if let Ok(uid) = user.parse::<u32>() {
            let pw = unsafe { libc::getpwuid(uid) };
            if pw.is_null() {
                return None;
            }
            // SAFETY: pw is a valid passwd pointer from getpwuid; we copy fields out.
            return Some(unsafe { passwd_fields(pw) });
        }
        let cname = std::ffi::CString::new(user).ok()?;
        let pw = unsafe { libc::getpwnam(cname.as_ptr()) };
        if pw.is_null() {
            return None;
        }
        // SAFETY: pw is a valid passwd pointer from getpwnam; we copy fields out.
        Some(unsafe { passwd_fields(pw) })
    }

    fn lookup_group(group: &str) -> Option<(u32, String)> {
        if let Ok(gid) = group.parse::<u32>() {
            let gr = unsafe { libc::getgrgid(gid) };
            if gr.is_null() {
                // Numeric gid is still usable for chown even without a name.
                return Some((gid, gid.to_string()));
            }
            let name = unsafe { CStr::from_ptr((*gr).gr_name) }
                .to_string_lossy()
                .into_owned();
            return Some((gid, name));
        }
        let cname = std::ffi::CString::new(group).ok()?;
        let gr = unsafe { libc::getgrnam(cname.as_ptr()) };
        if gr.is_null() {
            return None;
        }
        let gid = unsafe { (*gr).gr_gid };
        let name = unsafe { CStr::from_ptr((*gr).gr_name) }
            .to_string_lossy()
            .into_owned();
        Some((gid, name))
    }

    unsafe fn passwd_fields(pw: *mut libc::passwd) -> (u32, String, PathBuf, u32) {
        let uid = (*pw).pw_uid;
        let gid = (*pw).pw_gid;
        let name = CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned();
        let home = CStr::from_ptr((*pw).pw_dir).to_string_lossy().into_owned();
        (uid, name, PathBuf::from(home), gid)
    }

    fn username_for_uid(uid: u32) -> Option<String> {
        let pw = unsafe { libc::getpwuid(uid) };
        if pw.is_null() {
            return None;
        }
        let name = unsafe { CStr::from_ptr((*pw).pw_name) };
        Some(name.to_string_lossy().into_owned())
    }

    fn group_name_for_gid(gid: u32) -> Option<String> {
        let gr = unsafe { libc::getgrgid(gid) };
        if gr.is_null() {
            return None;
        }
        let name = unsafe { CStr::from_ptr((*gr).gr_name) };
        Some(name.to_string_lossy().into_owned())
    }
}

#[cfg(windows)]
mod platform {
    use super::LocalFileOwner;
    use std::path::PathBuf;

    pub(super) fn current_username() -> Option<String> {
        std::env::var("USERNAME")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    pub(super) fn lookup_owner(user: &str, group: Option<&str>) -> Option<LocalFileOwner> {
        // Validate the account exists (name or SID). Home is best-effort.
        if !account_exists(user) {
            return None;
        }
        if let Some(g) = group {
            if !account_exists(g) {
                tracing::warn!(group = %g, "output.local.owner_group not found; ignoring group");
            }
        }
        let home = resolve_home(user)?;
        let group = group.and_then(|g| account_exists(g).then(|| g.to_string()));
        Some(LocalFileOwner {
            user: user.to_string(),
            group,
            home,
        })
    }

    fn resolve_home(user: &str) -> Option<PathBuf> {
        if current_username().is_some_and(|u| u.eq_ignore_ascii_case(user)) {
            return std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty());
        }
        // SID form — cannot guess a profile path; require USERPROFILE when current.
        if user.starts_with("S-1-") {
            return std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty());
        }
        let bare = user.rsplit('\\').next().unwrap_or(user);
        let candidate = PathBuf::from(format!(r"C:\Users\{bare}"));
        if candidate.is_dir() {
            Some(candidate)
        } else {
            // Still allow ownership even if the profile path is non-standard;
            // `@user` expansion may be wrong — operators should set an absolute root.
            Some(candidate)
        }
    }

    fn account_exists(name: &str) -> bool {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr;
        use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
        use windows_sys::Win32::Security::{LookupAccountNameW, SidTypeUser};

        let wide: Vec<u16> = std::ffi::OsStr::new(name)
            .encode_wide()
            .chain(Some(0))
            .collect();

        // SID string → convert and look up.
        if name.starts_with("S-1-") {
            return sid_string_resolves(&wide);
        }

        unsafe {
            let mut sid_len = 0u32;
            let mut domain_len = 0u32;
            let mut sid_use = SidTypeUser;
            LookupAccountNameW(
                ptr::null(),
                wide.as_ptr(),
                ptr::null_mut(),
                &mut sid_len,
                ptr::null_mut(),
                &mut domain_len,
                &mut sid_use,
            );
            let err = windows_sys::Win32::Foundation::GetLastError();
            // First call is expected to fail with ERROR_INSUFFICIENT_BUFFER
            // once the required SID size is known.
            err == ERROR_INSUFFICIENT_BUFFER && sid_len > 0 || sid_len > 0
        }
    }

    fn sid_string_resolves(sid_wide: &[u16]) -> bool {
        use std::ptr;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
        use windows_sys::Win32::Security::{IsValidSid, LookupAccountSidW, SidTypeUser};

        unsafe {
            let mut sid = ptr::null_mut();
            if ConvertStringSidToSidW(sid_wide.as_ptr(), &mut sid) == 0 || sid.is_null() {
                return false;
            }
            let valid = IsValidSid(sid) != 0;
            let mut name_len = 0u32;
            let mut domain_len = 0u32;
            let mut sid_use = SidTypeUser;
            LookupAccountSidW(
                ptr::null(),
                sid,
                ptr::null_mut(),
                &mut name_len,
                ptr::null_mut(),
                &mut domain_len,
                &mut sid_use,
            );
            let ok = valid && name_len > 0;
            LocalFree(sid.cast());
            ok
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_user_sentinel() {
        let owner = LocalFileOwner {
            user: "alice".into(),
            group: None,
            home: PathBuf::from("/home/alice"),
            #[cfg(unix)]
            uid: 1000,
            #[cfg(unix)]
            gid: 1000,
        };
        assert_eq!(
            expand_user_local_root(Path::new("@user/Audiobooks"), &owner).unwrap(),
            PathBuf::from("/home/alice/Audiobooks")
        );
        assert_eq!(
            expand_user_local_root(Path::new("@user"), &owner).unwrap(),
            PathBuf::from("/home/alice/Audiobooks")
        );
        assert_eq!(
            expand_user_local_root(Path::new("@user/Music/Books"), &owner).unwrap(),
            PathBuf::from("/home/alice/Music/Books")
        );
        assert!(expand_user_local_root(Path::new("/data/Audiobooks"), &owner).is_none());
        assert!(is_user_local_root(Path::new("@user/Audiobooks")));
        assert!(!is_user_local_root(Path::new("Audiobooks")));
    }

    #[cfg(unix)]
    #[test]
    fn lookup_numeric_uid_for_current_user() {
        let uid = unsafe { libc::geteuid() };
        let owner = platform::lookup_owner(&uid.to_string(), None).expect("uid lookup");
        assert_eq!(owner.uid, uid);
        assert!(!owner.user.is_empty());
        assert!(!owner.home.as_os_str().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn lookup_numeric_gid() {
        let gid = unsafe { libc::getegid() };
        let uid = unsafe { libc::geteuid() };
        let owner =
            platform::lookup_owner(&uid.to_string(), Some(&gid.to_string())).expect("gid lookup");
        assert_eq!(owner.gid, gid);
    }
}
