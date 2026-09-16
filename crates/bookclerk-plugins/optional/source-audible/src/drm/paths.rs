//! Path validation before DRM filesystem sinks.

use std::path::{Path, PathBuf};

use super::error::{DrmError, Result};

/// Rejects empty paths and interior `..` / NUL, then rebuilds for FS sinks.
pub(crate) fn validated_fs_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(DrmError::Native("refusing empty path".into()));
    }
    let s = path.to_string_lossy();
    if s.contains("..") || s.contains('\0') {
        return Err(DrmError::Native(format!(
            "refusing unsafe path: {}",
            path.display()
        )));
    }
    Ok(PathBuf::from(s.into_owned()))
}
