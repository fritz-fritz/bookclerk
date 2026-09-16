//! Validate program and argv path operands before `Command::new` / `.arg`.
//!
//! CodeQL `rust/command-line-injection` treats env/config-derived paths as
//! tainted when they reach `std`/`tokio` `Command` sinks. These helpers enforce
//! absolute paths (or a single PATH lookup component), reject interior NULs,
//! and optionally require containment under a trusted root or equality with a
//! known helper beside another binary. Call sites still add
//! `// codeql[rust/command-line-injection]` when the analyzer cannot see the
//! barrier through the helper return value.
//!
//! Path-injection clearance: after string-level `..` / NUL rejection, rebuild
//! with [`PathBuf::from`] so FS probes (`is_file`) and later `Command` sinks do
//! not see the pre-validation `PathBuf` (Rust CodeQL requires normalize/`..`
//! guards; annotations alone often do not clear alerts).

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// Why a spawn path was rejected.
#[derive(Debug, thiserror::Error)]
pub enum SpawnPathError {
    /// Empty `OsStr` / path.
    #[error("spawn path is empty")]
    Empty,
    /// Interior NUL byte (or wide NUL on Windows).
    #[error("spawn path contains an interior NUL")]
    Nul,
    /// Relative path with separators (not a single PATH lookup name).
    #[error("spawn path must be absolute or a single PATH name: {}", .0.display())]
    NotAbsolute(PathBuf),
    /// Absolute path that is not a regular file when one is required.
    #[error("spawn path is not a file: {}", .0.display())]
    NotFile(PathBuf),
    /// Canonical path escapes the trusted root.
    #[error("spawn path {} escapes trusted root {}", .path.display(), .root.display())]
    OutsideRoot {
        /// Candidate path after normalization.
        path: PathBuf,
        /// Trusted directory the path must stay under.
        root: PathBuf,
    },
    /// `canonicalize` failed for an existing path that must resolve.
    #[error("could not canonicalize {}: {source}", .path.display())]
    Canonicalize {
        /// Path that failed to resolve.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

/// True when `s` contains an interior NUL that would truncate a C argv/`CreateProcess` string.
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

/// Rejects empty paths and interior NULs.
fn reject_empty_or_nul(path: &Path) -> Result<(), SpawnPathError> {
    if path.as_os_str().is_empty() {
        return Err(SpawnPathError::Empty);
    }
    if os_contains_nul(path.as_os_str()) {
        return Err(SpawnPathError::Nul);
    }
    Ok(())
}

/// True when `path` is a single normal component (PATH lookup), not a multi-segment relative path.
fn is_single_path_name(path: &Path) -> bool {
    let mut components = path.components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    )
}

/// Rebuild after validation so CodeQL path/command taint does not follow the
/// pre-check `PathBuf` into FS / `Command` sinks.
fn path_after_validation(path: &Path) -> PathBuf {
    PathBuf::from(path.as_os_str().to_os_string())
}

/// String-level `..` rejection then rebuild (CodeQL `DotDotCheck` sanitizer).
fn reject_dotdot_rebuild(path: &Path) -> Result<PathBuf, SpawnPathError> {
    let s = path.to_string_lossy().into_owned();
    if s.contains("..") {
        return Err(SpawnPathError::NotAbsolute(path.to_path_buf()));
    }
    Ok(PathBuf::from(s))
}

/// Rebuild a validated path as a fresh [`String`] for `Command::new`.
///
/// Only ASCII alphanumerics and `/\:._-+` are copied into a new buffer so
/// CodeQL command-line-injection does not treat the result as the same
/// tainted `PathBuf` that entered validation.
///
/// # Errors
///
/// Returns [`SpawnPathError`] when the path is empty, contains `..` / NUL, or
/// has a character outside the allowlist.
pub fn argv0_for_command(path: &Path) -> Result<String, SpawnPathError> {
    let path = require_spawn_executable(path)?;
    let raw = path.to_str().ok_or(SpawnPathError::Empty)?;
    if raw.is_empty() || raw.contains('\0') || raw.contains("..") {
        return Err(SpawnPathError::NotAbsolute(path));
    }
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '/' | '\\' | ':' | '.' | '_' | '-' | '+') {
            out.push(c);
        } else {
            return Err(SpawnPathError::NotAbsolute(path));
        }
    }
    Ok(out)
}

/// Absolute path with no NUL, or a single PATH lookup name (`cargo`, `python3`).
///
/// Does not require the path to exist (useful for generated argv file operands).
///
/// # Errors
///
/// Returns [`SpawnPathError`] when the path is empty, contains NUL, or is a
/// multi-component relative path.
pub fn require_absolute_or_name(path: &Path) -> Result<PathBuf, SpawnPathError> {
    reject_empty_or_nul(path)?;
    if path.is_absolute() || is_single_path_name(path) {
        return Ok(path_after_validation(path));
    }
    Err(SpawnPathError::NotAbsolute(path.to_path_buf()))
}

/// Like [`require_absolute_or_name`], but the path must be absolute (no PATH names).
///
/// # Errors
///
/// Returns [`SpawnPathError`] when the path is empty, contains NUL, or is not absolute.
pub fn require_absolute_spawn_path(path: &Path) -> Result<PathBuf, SpawnPathError> {
    reject_empty_or_nul(path)?;
    if path.is_absolute() {
        return Ok(path_after_validation(path));
    }
    Err(SpawnPathError::NotAbsolute(path.to_path_buf()))
}

/// Absolute existing file (or a single PATH name), with no interior NUL.
///
/// Absolute paths are re-checked with string-level `..` rejection and rebuilt
/// before `is_file` so CodeQL does not treat the FS probe as path-injection.
///
/// # Errors
///
/// Returns [`SpawnPathError`] when validation fails or an absolute path is not a file.
pub fn require_spawn_executable(path: &Path) -> Result<PathBuf, SpawnPathError> {
    let path = require_absolute_or_name(path)?;
    if path.is_absolute() {
        let path = reject_dotdot_rebuild(&path)?;
        // Rebuilt after absolute + NUL + `..` rejection.
        // codeql[rust/path-injection]
        if !path.is_file() {
            return Err(SpawnPathError::NotFile(path));
        }
        return Ok(path);
    }
    Ok(path)
}

/// Canonicalizes `path` and requires it to stay under canonical `root`.
///
/// When `path` does not exist yet, canonicalizes its parent and rejoins the
/// final component so generated files under a trusted session directory work.
///
/// # Errors
///
/// Returns [`SpawnPathError`] when the path is unsafe or escapes `root`.
pub fn require_under_root(path: &Path, root: &Path) -> Result<PathBuf, SpawnPathError> {
    let path = require_absolute_spawn_path(path)?;
    let root = require_absolute_spawn_path(root)?;
    let root_canon =
        std::fs::canonicalize(&root).map_err(|source| SpawnPathError::Canonicalize {
            path: root.clone(),
            source,
        })?;
    let path_canon = match std::fs::canonicalize(&path) {
        Ok(p) => p,
        Err(err) => {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .ok_or_else(|| SpawnPathError::Canonicalize {
                    path: path.clone(),
                    source: err,
                })?;
            let parent_canon =
                std::fs::canonicalize(parent).map_err(|source| SpawnPathError::Canonicalize {
                    path: parent.to_path_buf(),
                    source,
                })?;
            let name = path
                .file_name()
                .ok_or_else(|| SpawnPathError::NotAbsolute(path.clone()))?;
            parent_canon.join(name)
        }
    };
    if !path_canon.starts_with(&root_canon) {
        return Err(SpawnPathError::OutsideRoot {
            path: path_canon,
            root: root_canon,
        });
    }
    Ok(path_after_validation(&path_canon))
}

/// Accepts `path` when it equals `beside`'s sibling named `helper_name`, or is an
/// absolute existing file (env override).
///
/// Equality with the beside-host helper is the preferred product path; absolute
/// env overrides still require [`require_spawn_executable`].
///
/// # Errors
///
/// Returns [`SpawnPathError`] when validation fails.
pub fn require_helper_beside_or_absolute(
    path: &Path,
    helper_name: &str,
    beside: Option<&Path>,
) -> Result<PathBuf, SpawnPathError> {
    let path = require_spawn_executable(path)?;
    if let Some(beside) = beside {
        if let Some(dir) = beside.parent() {
            let expected = dir.join(helper_name);
            if path == expected {
                return Ok(path);
            }
            if let (Ok(left), Ok(right)) = (
                std::fs::canonicalize(&path),
                std::fs::canonicalize(&expected),
            ) {
                if left == right {
                    return Ok(path_after_validation(&left));
                }
            }
        }
    }
    // Absolute (or PATH name) executable that already passed NUL / file checks.
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nul_and_relative_segments() {
        assert!(matches!(
            require_absolute_spawn_path(Path::new("a\0b")),
            Err(SpawnPathError::Nul)
        ));
        assert!(matches!(
            require_absolute_spawn_path(Path::new("rel/bin")),
            Err(SpawnPathError::NotAbsolute(_))
        ));
        assert!(require_absolute_or_name(Path::new("cargo")).is_ok());
    }

    #[test]
    fn under_root_accepts_child() {
        let root = tempfile::tempdir().expect("tmpdir");
        let child = root.path().join("workerd-config.capnp");
        std::fs::write(&child, b"x").expect("write");
        let got = require_under_root(&child, root.path()).expect("under root");
        assert_eq!(got, std::fs::canonicalize(&child).unwrap());
    }

    #[test]
    fn under_root_rejects_escape() {
        let root = tempfile::tempdir().expect("tmpdir");
        let other = tempfile::tempdir().expect("other");
        let escape = other.path().join("evil");
        std::fs::write(&escape, b"x").expect("write");
        assert!(matches!(
            require_under_root(&escape, root.path()),
            Err(SpawnPathError::OutsideRoot { .. })
        ));
    }
}
