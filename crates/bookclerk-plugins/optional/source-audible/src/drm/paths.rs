//! Empty/NUL checks before DRM filesystem sinks.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::error::{DrmError, Result};

/// True when `s` contains an interior NUL.
fn os_contains_nul(s: &OsStr) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        s.as_bytes().contains(&0)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide().any(|c| c == 0)
    }
}

/// Rejects empty paths and interior NULs; returns `path` unchanged otherwise.
///
/// Download/output paths are caller-selected under the acquire cache, not
/// untrusted suffixes under a separate root. Do not reject lexical `..` or
/// convert through UTF-8 lossy here.
///
/// # Errors
///
/// Returns [`DrmError::Native`] when the path is empty or contains NUL.
pub(crate) fn validated_fs_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(DrmError::Native("refusing empty path".into()));
    }
    if os_contains_nul(path.as_os_str()) {
        return Err(DrmError::Native(format!(
            "refusing path with interior NUL: {}",
            path.display()
        )));
    }
    Ok(path.to_path_buf())
}
