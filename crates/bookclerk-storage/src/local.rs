//! Local filesystem storage backend.

#![allow(unsafe_code)] // seteuid / SetNamedSecurityInfo / privilege adjust

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use async_trait::async_trait;
use bytes::Bytes;
use filetime::{set_file_times, FileTime};
use tokio::fs;

use crate::error::{Result, StorageError};
use crate::normalize_prefix;
use crate::traits::{
    bookclerk_meta_sidecar_key, ObjectInfo, ObjectMeta, ObjectProbe, StorageBackend,
};

/// OS identity applied to created files and directories after write.
///
/// Unix: numeric uid/gid (`chown`). Windows: account name or `S-1-…` SID
/// (`SetNamedSecurityInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFsOwner {
    #[cfg(unix)]
    pub uid: u32,
    #[cfg(unix)]
    pub gid: u32,
    /// Windows owner account name or SID string.
    #[cfg(windows)]
    pub user: String,
    /// Optional Windows group name or SID string.
    #[cfg(windows)]
    pub group: Option<String>,
}

/// Stores objects under a root directory; keys map to relative paths.
///
/// An optional [`Self::prefix`] is prepended to every key (same model as S3),
/// so library `storage_key` values stay relative to the prefix.
#[derive(Debug, Clone)]
pub struct LocalFsBackend {
    root: PathBuf,
    prefix: String,
    owner: Option<LocalFsOwner>,
}

impl LocalFsBackend {
    /// Create a backend rooted at `root` with no key prefix.
    pub fn new(root: PathBuf) -> Result<Self> {
        Self::with_prefix(root, "")
    }

    /// Create a backend rooted at `root` with an optional key prefix
    /// (e.g. `library/`). The prefix directory is created when needed.
    pub fn with_prefix(root: PathBuf, prefix: &str) -> Result<Self> {
        Self::with_prefix_and_owner(root, prefix, None)
    }

    /// Like [`Self::with_prefix`], and optionally set ownership on new files.
    pub fn with_prefix_and_owner(
        root: PathBuf,
        prefix: &str,
        owner: Option<LocalFsOwner>,
    ) -> Result<Self> {
        let prefix = normalize_prefix(prefix);
        std::fs::create_dir_all(&root)?;
        chown_path(&root, owner.as_ref())?;
        if !prefix.is_empty() {
            let pref_dir = root.join(prefix.trim_end_matches('/'));
            std::fs::create_dir_all(&pref_dir)?;
            chown_path(&pref_dir, owner.as_ref())?;
        }
        Ok(Self {
            root,
            prefix,
            owner,
        })
    }

    fn full_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}{key}", self.prefix)
        }
    }

    fn resolve(&self, key: &str) -> Result<PathBuf> {
        validate_key(key)?;
        let full = self.full_key(key);
        // full_key only prepends a normalized prefix; still reject escape in the
        // combined path.
        if full.contains("..") {
            return Err(StorageError::InvalidKey(key.into()));
        }
        let path = self.root.join(&full);
        // Prevent path escape above root.
        let canonical_root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        if let Ok(canonical) = path.canonicalize() {
            if !canonical.starts_with(&canonical_root) {
                return Err(StorageError::InvalidKey(key.into()));
            }
        } else {
            // Parent must still stay under root when the file does not exist yet.
            if let Some(parent) = path.parent() {
                let parent_canon = if parent.exists() {
                    parent
                        .canonicalize()
                        .unwrap_or_else(|_| parent.to_path_buf())
                } else {
                    parent.to_path_buf()
                };
                if parent_canon.is_absolute()
                    && !parent_canon.starts_with(&canonical_root)
                    && !path.starts_with(&self.root)
                {
                    return Err(StorageError::InvalidKey(key.into()));
                }
            }
        }
        Ok(path)
    }

    fn chown_tree_to(&self, path: &Path) -> Result<()> {
        let Some(owner) = self.owner.as_ref() else {
            return Ok(());
        };
        // Chown the file and every parent under root (best-effort).
        let mut cur = path.to_path_buf();
        loop {
            chown_path(&cur, Some(owner))?;
            if cur == self.root {
                break;
            }
            match cur.parent() {
                Some(p) if p.starts_with(&self.root) || p == self.root => {
                    cur = p.to_path_buf();
                }
                _ => break,
            }
        }
        Ok(())
    }
}

fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() || key.starts_with('/') || key.contains("..") {
        return Err(StorageError::InvalidKey(key.into()));
    }
    Ok(())
}

fn chown_path(path: &Path, owner: Option<&LocalFsOwner>) -> Result<()> {
    let Some(owner) = owner else {
        return Ok(());
    };
    #[cfg(unix)]
    {
        chown_unix(path, owner)?;
    }
    #[cfg(windows)]
    {
        chown_windows(path, owner)?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, owner);
    }
    Ok(())
}

#[cfg(unix)]
fn chown_unix(path: &Path, owner: &LocalFsOwner) -> Result<()> {
    use std::os::unix::fs::chown;

    // macOS: after privilege drop we keep real uid 0 and only seteuid to the
    // service account — briefly restore euid 0 around chown.
    #[cfg(target_os = "macos")]
    let _elevated = MacOsRootElevation::try_elevate();

    if let Err(err) = chown(path, Some(owner.uid), Some(owner.gid)) {
        if err.kind() != std::io::ErrorKind::PermissionDenied {
            return Err(StorageError::Io(err));
        }
        tracing::debug!(
            path = %path.display(),
            uid = owner.uid,
            gid = owner.gid,
            "chown skipped (permission denied)"
        );
    }
    Ok(())
}

/// RAII helper: `seteuid(0)` while real uid is still root (macOS drop path).
#[cfg(target_os = "macos")]
struct MacOsRootElevation {
    saved_euid: u32,
}

#[cfg(target_os = "macos")]
impl MacOsRootElevation {
    fn try_elevate() -> Option<Self> {
        // SAFETY: getuid/geteuid/seteuid are process-wide but only used around
        // a short chown critical section on the acquire path.
        let ruid = unsafe { libc::getuid() };
        let euid = unsafe { libc::geteuid() };
        if ruid != 0 || euid == 0 {
            return None;
        }
        if unsafe { libc::seteuid(0) } != 0 {
            tracing::debug!(
                err = %std::io::Error::last_os_error(),
                "macOS seteuid(0) for chown failed"
            );
            return None;
        }
        Some(Self { saved_euid: euid })
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacOsRootElevation {
    fn drop(&mut self) {
        // SAFETY: restore the service euid after temporary root for chown.
        if unsafe { libc::seteuid(self.saved_euid) } != 0 {
            tracing::error!(
                err = %std::io::Error::last_os_error(),
                saved_euid = self.saved_euid,
                "failed to restore euid after macOS chown elevation"
            );
        }
    }
}

#[cfg(windows)]
fn chown_windows(path: &Path, owner: &LocalFsOwner) -> Result<()> {
    // Best-effort: enable SeRestorePrivilege / SeTakeOwnershipPrivilege when
    // available, then SetNamedSecurityInfo for owner (+ optional group).
    enable_ownership_privileges();
    match set_file_owner(path, &owner.user, owner.group.as_deref()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            tracing::debug!(
                path = %path.display(),
                user = %owner.user,
                "SetNamedSecurityInfo skipped (permission denied)"
            );
            Ok(())
        }
        Err(err) => {
            tracing::debug!(
                path = %path.display(),
                user = %owner.user,
                %err,
                "SetNamedSecurityInfo failed"
            );
            Ok(())
        }
    }
}

#[cfg(windows)]
fn enable_ownership_privileges() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        for name in ["SeRestorePrivilege", "SeTakeOwnershipPrivilege"] {
            if let Err(err) = enable_privilege(name) {
                tracing::debug!(privilege = name, %err, "could not enable ownership privilege");
            }
        }
    });
}

#[cfg(windows)]
fn enable_privilege(name: &str) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_NOT_ALL_ASSIGNED, ERROR_SUCCESS, HANDLE, LUID,
    };
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let wide: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        ) == 0
        {
            return Err(std::io::Error::from_raw_os_error(GetLastError() as i32));
        }
        let mut luid = LUID {
            LowPart: 0,
            HighPart: 0,
        };
        if LookupPrivilegeValueW(ptr::null(), wide.as_ptr(), &mut luid) == 0 {
            let err = GetLastError();
            CloseHandle(token);
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let ok = AdjustTokenPrivileges(token, 0, &tp, 0, ptr::null_mut(), ptr::null_mut());
        let err = GetLastError();
        CloseHandle(token);
        if ok == 0 {
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
        // Success with ERROR_NOT_ALL_ASSIGNED means the privilege is not held.
        if err == ERROR_NOT_ALL_ASSIGNED {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("privilege {name} not held"),
            ));
        }
        if err != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn set_file_owner(path: &Path, user: &str, group: Option<&str>) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS};
    use windows_sys::Win32::Security::Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSID,
    };

    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let owner_sid = resolve_sid(user)?;
    let group_sid = match group {
        Some(g) => Some(resolve_sid(g)?),
        None => None,
    };

    let mut info = OWNER_SECURITY_INFORMATION;
    if group_sid.is_some() {
        info |= GROUP_SECURITY_INFORMATION;
    }

    let owner_psid: PSID = owner_sid.as_ptr();
    let group_psid: PSID = group_sid
        .as_ref()
        .map_or(ptr::null_mut(), ResolvedSid::as_ptr);
    // SAFETY: path_wide / SIDs live for the duration of the call.
    let status = unsafe {
        SetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            info,
            owner_psid,
            group_psid,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    owner_sid.free_if_local();
    if let Some(g) = group_sid {
        g.free_if_local();
    }
    if status != ERROR_SUCCESS {
        let kind = if status == ERROR_ACCESS_DENIED {
            std::io::ErrorKind::PermissionDenied
        } else {
            std::io::ErrorKind::Other
        };
        return Err(std::io::Error::new(
            kind,
            format!("SetNamedSecurityInfoW failed: {status}"),
        ));
    }
    Ok(())
}

#[cfg(windows)]
struct ResolvedSid {
    /// Owned buffer for LookupAccountName results.
    buf: Option<Vec<u8>>,
    /// Pointer from ConvertStringSidToSidW (LocalFree).
    local: Option<*mut core::ffi::c_void>,
}

#[cfg(windows)]
impl ResolvedSid {
    fn as_ptr(&self) -> windows_sys::Win32::Security::PSID {
        if let Some(local) = self.local {
            return local;
        }
        self.buf
            .as_ref()
            .map_or(std::ptr::null_mut(), |b| b.as_ptr().cast_mut().cast())
    }

    fn free_if_local(self) {
        if let Some(local) = self.local {
            unsafe {
                windows_sys::Win32::Foundation::LocalFree(local);
            }
        }
    }
}

#[cfg(windows)]
fn resolve_sid(name: &str) -> std::io::Result<ResolvedSid> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
    use windows_sys::Win32::Security::{LookupAccountNameW, SidTypeUser};

    let wide: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(Some(0))
        .collect();

    if name.starts_with("S-1-") {
        unsafe {
            let mut sid = ptr::null_mut();
            if ConvertStringSidToSidW(wide.as_ptr(), &mut sid) == 0 || sid.is_null() {
                return Err(std::io::Error::from_raw_os_error(GetLastError() as i32));
            }
            return Ok(ResolvedSid {
                buf: None,
                local: Some(sid),
            });
        }
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
        if sid_len == 0 {
            return Err(std::io::Error::from_raw_os_error(GetLastError() as i32));
        }
        let mut sid_buf = vec![0u8; sid_len as usize];
        let mut domain = vec![0u16; domain_len.max(1) as usize];
        if LookupAccountNameW(
            ptr::null(),
            wide.as_ptr(),
            sid_buf.as_mut_ptr().cast(),
            &mut sid_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut sid_use,
        ) == 0
        {
            return Err(std::io::Error::from_raw_os_error(GetLastError() as i32));
        }
        Ok(ResolvedSid {
            buf: Some(sid_buf),
            local: None,
        })
    }
}

#[async_trait]
impl StorageBackend for LocalFsBackend {
    fn name(&self) -> &'static str {
        "local"
    }

    fn clone_box(&self) -> Box<dyn StorageBackend> {
        Box::new(self.clone())
    }

    async fn put(&self, key: &str, data: Bytes, meta: ObjectMeta) -> Result<()> {
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
            self.chown_tree_to(parent)?;
        }
        fs::write(&path, &data).await?;
        self.chown_tree_to(&path)?;
        write_local_meta_sidecar(self, key, &meta).await?;
        Ok(())
    }

    async fn put_file(&self, key: &str, source: &Path, meta: ObjectMeta) -> Result<()> {
        let dest = self.resolve(key)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).await?;
            self.chown_tree_to(parent)?;
        }
        // Prefer hard-link/copy without loading the whole audiobook into RAM.
        match fs::hard_link(source, &dest).await {
            Ok(()) => {}
            Err(_) => {
                fs::copy(source, &dest).await?;
            }
        }
        self.chown_tree_to(&dest)?;
        write_local_meta_sidecar(self, key, &meta).await?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Bytes> {
        let path = self.resolve(key)?;
        let data = fs::read(&path).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.into())
            } else {
                StorageError::Io(err)
            }
        })?;
        Ok(Bytes::from(data))
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let path = self.resolve(key)?;
        Ok(fs::try_exists(&path).await?)
    }

    async fn list(&self, prefix: &str) -> Result<Vec<ObjectInfo>> {
        validate_key(prefix).or_else(|_| {
            if prefix.is_empty() {
                Ok(())
            } else {
                Err(StorageError::InvalidKey(prefix.into()))
            }
        })?;
        let mut out = Vec::new();
        let full_prefix = self.full_key(prefix);
        list_recursive(&self.root, &self.root, &full_prefix, &mut out).await?;
        // Strip the storage prefix so returned keys match library storage_key
        // values (same as S3Backend).
        if !self.prefix.is_empty() {
            out = out
                .into_iter()
                .filter_map(|obj| {
                    obj.key.strip_prefix(&self.prefix).map(|rest| ObjectInfo {
                        key: rest.to_string(),
                        size: obj.size,
                    })
                })
                .collect();
        }
        Ok(out)
    }

    async fn probe(&self, key: &str) -> Result<ObjectProbe> {
        let path = self.resolve(key)?;
        let file_meta = fs::metadata(&path).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.into())
            } else {
                StorageError::Io(err)
            }
        })?;
        let mut probe = ObjectProbe {
            key: key.to_string(),
            size: file_meta.len(),
            content_type: None,
            meta: ObjectMeta {
                content_length: Some(file_meta.len()),
                ..Default::default()
            },
        };
        // Cheap sidecar read — never opens the audio body.
        let meta_key = bookclerk_meta_sidecar_key(key);
        if let Ok(bytes) = self.get(&meta_key).await {
            if let Ok(parsed) = serde_json::from_slice::<ObjectMeta>(&bytes) {
                probe.meta.asin = parsed.asin.or(probe.meta.asin);
                probe.meta.title = parsed.title.or(probe.meta.title);
                probe.meta.creation_time = parsed.creation_time.or(probe.meta.creation_time);
                probe.meta.last_write_time = parsed.last_write_time.or(probe.meta.last_write_time);
                probe.content_type = parsed.content_type.or(probe.content_type);
                if parsed.content_length.is_some() {
                    probe.meta.content_length = parsed.content_length;
                }
            }
        }
        Ok(probe)
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        if from == to {
            return Ok(());
        }
        let src = self.resolve(from)?;
        let dest = self.resolve(to)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).await?;
            self.chown_tree_to(parent)?;
        }
        fs::copy(&src, &dest).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(from.into())
            } else {
                StorageError::Io(err)
            }
        })?;
        self.chown_tree_to(&dest)?;
        // Move companion meta sidecar when present.
        let from_meta = bookclerk_meta_sidecar_key(from);
        let to_meta = bookclerk_meta_sidecar_key(to);
        if self.exists(&from_meta).await? {
            let meta_src = self.resolve(&from_meta)?;
            let meta_dest = self.resolve(&to_meta)?;
            if let Some(parent) = meta_dest.parent() {
                fs::create_dir_all(parent).await?;
                self.chown_tree_to(parent)?;
            }
            let _ = fs::copy(&meta_src, &meta_dest).await;
            let _ = self.chown_tree_to(&meta_dest);
        }
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.resolve(key)?;
        match fs::remove_file(&path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(StorageError::Io(err)),
        }
        // Best-effort remove companion meta when deleting the primary object.
        if !key.ends_with(".bookclerk-meta.json") {
            let meta_key = bookclerk_meta_sidecar_key(key);
            let _ = self.delete(&meta_key).await;
        }
        Ok(())
    }

    async fn touch_file(
        &self,
        key: &str,
        created: Option<SystemTime>,
        modified: Option<SystemTime>,
    ) -> Result<()> {
        let path = self.resolve(key)?;
        if !path.exists() {
            return Ok(());
        }
        let created = created.map(FileTime::from_system_time);
        let modified = modified.map(FileTime::from_system_time);
        match (created, modified) {
            (Some(c), Some(m)) => set_file_times(&path, c, m).map_err(StorageError::Io)?,
            (None, Some(m)) => {
                let meta = std::fs::metadata(&path).map_err(StorageError::Io)?;
                let c = FileTime::from_last_modification_time(&meta);
                set_file_times(&path, c, m).map_err(StorageError::Io)?;
            }
            (Some(c), None) => {
                let meta = std::fs::metadata(&path).map_err(StorageError::Io)?;
                let m = FileTime::from_last_modification_time(&meta);
                set_file_times(&path, c, m).map_err(StorageError::Io)?;
            }
            (None, None) => {}
        }
        Ok(())
    }
}

async fn list_recursive(
    root: &Path,
    dir: &Path,
    prefix: &str,
    out: &mut Vec<ObjectInfo>,
) -> Result<()> {
    let mut read_dir = match fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(StorageError::Io(err)),
    };

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            Box::pin(list_recursive(root, &path, prefix, out)).await?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|_| StorageError::InvalidKey(path.display().to_string()))?;
        let key = rel.to_string_lossy().replace('\\', "/");
        if !prefix.is_empty() && !key.starts_with(prefix) {
            continue;
        }
        let meta = fs::metadata(&path).await?;
        out.push(ObjectInfo {
            key,
            size: meta.len(),
        });
    }
    Ok(())
}

async fn write_local_meta_sidecar(
    backend: &LocalFsBackend,
    key: &str,
    meta: &ObjectMeta,
) -> Result<()> {
    // Skip recursive meta-for-meta; only persist meaningful identity tags.
    if key.ends_with(".bookclerk-meta.json") {
        return Ok(());
    }
    if meta.asin.is_none() && meta.title.is_none() {
        return Ok(());
    }
    let sidecar = bookclerk_meta_sidecar_key(key);
    let payload =
        serde_json::to_vec(meta).map_err(|err| StorageError::Io(std::io::Error::other(err)))?;
    let path = backend.resolve(&sidecar)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
        backend.chown_tree_to(parent)?;
    }
    fs::write(&path, payload).await?;
    backend.chown_tree_to(&path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn put_get_exists_delete() {
        let dir = tempdir().unwrap();
        let backend = LocalFsBackend::new(dir.path().to_path_buf()).unwrap();
        let key = "Author/Title/book.m4b";
        assert!(!backend.exists(key).await.unwrap());
        backend
            .put(
                key,
                Bytes::from_static(b"audio"),
                ObjectMeta {
                    asin: Some("B00X".into()),
                    title: Some("Book".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(backend.exists(key).await.unwrap());
        assert_eq!(backend.get(key).await.unwrap().as_ref(), b"audio");
        let probe = backend.probe(key).await.unwrap();
        assert_eq!(probe.meta.asin.as_deref(), Some("B00X"));
        assert_eq!(probe.meta.title.as_deref(), Some("Book"));
        let listed = backend.list_audio("").await.unwrap();
        assert_eq!(listed.len(), 1);
        backend.delete(key).await.unwrap();
        assert!(!backend.exists(key).await.unwrap());
    }

    #[tokio::test]
    async fn rename_moves_audio_and_meta_sidecar() {
        let dir = tempdir().unwrap();
        let backend = LocalFsBackend::new(dir.path().to_path_buf()).unwrap();
        backend
            .put(
                "Old/book.m4b",
                Bytes::from_static(b"audio"),
                ObjectMeta {
                    asin: Some("B00X".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        backend
            .rename("Old/book.m4b", "New/book.m4b")
            .await
            .unwrap();
        assert!(!backend.exists("Old/book.m4b").await.unwrap());
        assert!(backend.exists("New/book.m4b").await.unwrap());
        let probe = backend.probe("New/book.m4b").await.unwrap();
        assert_eq!(probe.meta.asin.as_deref(), Some("B00X"));
        assert!(!backend
            .exists(&bookclerk_meta_sidecar_key("Old/book.m4b"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn put_file_copies_without_bytes_api() {
        let dir = tempdir().unwrap();
        let backend = LocalFsBackend::new(dir.path().join("store")).unwrap();
        let src = dir.path().join("src.m4b");
        std::fs::write(&src, b"from-file").unwrap();
        backend
            .put_file("A/B.m4b", &src, ObjectMeta::default())
            .await
            .unwrap();
        assert_eq!(backend.get("A/B.m4b").await.unwrap().as_ref(), b"from-file");
    }

    #[tokio::test]
    async fn rejects_path_escape() {
        let dir = tempdir().unwrap();
        let backend = LocalFsBackend::new(dir.path().to_path_buf()).unwrap();
        let err = backend
            .put(
                "../escape.m4b",
                Bytes::from_static(b"x"),
                ObjectMeta::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::InvalidKey(_)));
    }

    #[tokio::test]
    async fn prefix_scopes_keys_under_root() {
        let dir = tempdir().unwrap();
        let backend = LocalFsBackend::with_prefix(dir.path().to_path_buf(), "library/").unwrap();
        backend
            .put(
                "Author/Book.m4b",
                Bytes::from_static(b"audio"),
                ObjectMeta {
                    asin: Some("B00X".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(dir.path().join("library/Author/Book.m4b").is_file());
        assert!(backend.exists("Author/Book.m4b").await.unwrap());
        let listed = backend.list("").await.unwrap();
        assert!(
            listed.iter().any(|o| o.key == "Author/Book.m4b"),
            "list should return keys relative to prefix: {listed:?}"
        );
        assert!(
            !listed.iter().any(|o| o.key.starts_with("library/")),
            "list must strip storage prefix from returned keys"
        );
        // Objects outside the prefix are invisible.
        std::fs::write(dir.path().join("other.m4b"), b"nope").unwrap();
        let listed = backend.list_audio("").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, "Author/Book.m4b");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn owner_chown_applies_when_permitted() {
        let dir = tempdir().unwrap();
        let uid = unsafe { libc::geteuid() };
        let gid = unsafe { libc::getegid() };
        let backend = LocalFsBackend::with_prefix_and_owner(
            dir.path().to_path_buf(),
            "",
            Some(LocalFsOwner { uid, gid }),
        )
        .unwrap();
        backend
            .put("a/b.m4b", Bytes::from_static(b"x"), ObjectMeta::default())
            .await
            .unwrap();
        let meta = std::fs::metadata(dir.path().join("a/b.m4b")).unwrap();
        use std::os::unix::fs::MetadataExt;
        assert_eq!(meta.uid(), uid);
        assert_eq!(meta.gid(), gid);
    }
}
