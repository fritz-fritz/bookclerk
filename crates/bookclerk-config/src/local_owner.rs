//! Resolve who should own local acquired audiobooks (uid/gid + home).

#![allow(unsafe_code)] // getpwnam / getpwuid on Unix

use std::path::{Path, PathBuf};

use crate::output::{OutputLocalConfig, OUTPUT_LOCAL_USER_ROOT};

/// Resolved OS identity for `[output.local]` ownership and `@user/…` roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFileOwner {
    pub user: String,
    pub group: Option<String>,
    pub home: PathBuf,
    #[cfg(unix)]
    pub uid: u32,
    #[cfg(unix)]
    pub gid: u32,
}

/// Resolve the file owner for local output.
///
/// Order: `output.local.owner_user` → `BOOKCLERK_OUTPUT_OWNER` → `SUDO_USER` →
/// current interactive user (skips `root` / `bookclerk`).
#[must_use]
pub fn resolve_local_file_owner(local: &OutputLocalConfig) -> Option<LocalFileOwner> {
    let name = local
        .owner_user
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| env_nonempty("BOOKCLERK_OUTPUT_OWNER"))
        .or_else(|| env_nonempty("SUDO_USER"))
        .or_else(interactive_username)?;

    let group = local
        .owner_group
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| env_nonempty("BOOKCLERK_OUTPUT_OWNER_GROUP"));

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
        let cname = std::ffi::CString::new(user).ok()?;
        let pw = unsafe { libc::getpwnam(cname.as_ptr()) };
        if pw.is_null() {
            return None;
        }
        let uid = unsafe { (*pw).pw_uid };
        let mut gid = unsafe { (*pw).pw_gid };
        let home = unsafe { CStr::from_ptr((*pw).pw_dir) }
            .to_string_lossy()
            .into_owned();
        let mut group_name = group.map(str::to_string);
        if let Some(gname) = group {
            if let Ok(cg) = std::ffi::CString::new(gname) {
                let gr = unsafe { libc::getgrnam(cg.as_ptr()) };
                if !gr.is_null() {
                    gid = unsafe { (*gr).gr_gid };
                    group_name = Some(gname.to_string());
                }
            }
        } else {
            group_name = group_name_for_gid(gid);
        }
        Some(LocalFileOwner {
            user: user.to_string(),
            group: group_name,
            home: PathBuf::from(home),
            uid,
            gid,
        })
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
        // Prefer the current profile when it matches; otherwise USERPROFILE-style
        // guess under C:\Users\<user>.
        let home = if current_username().is_some_and(|u| u.eq_ignore_ascii_case(user)) {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty())?
        } else {
            PathBuf::from(format!(r"C:\Users\{user}"))
        };
        Some(LocalFileOwner {
            user: user.to_string(),
            group: group.map(str::to_string),
            home,
        })
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
}
