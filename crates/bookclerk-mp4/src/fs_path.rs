//! Path validation before MP4 filesystem sinks.

use std::path::{Path, PathBuf};

use crate::error::{Mp4Error, Result};

/// Rejects empty paths and interior `..` / NUL, then rebuilds for FS sinks.
pub(crate) fn validated(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(Mp4Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing empty path",
        )));
    }
    let s = path.to_string_lossy();
    if s.contains("..") || s.contains('\0') {
        return Err(Mp4Error::container(format!(
            "refusing unsafe path: {}",
            path.display()
        )));
    }
    Ok(PathBuf::from(s.into_owned()))
}
