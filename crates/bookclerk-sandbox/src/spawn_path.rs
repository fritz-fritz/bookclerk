//! Validate program and argv path operands before `Command::new` / `.arg`.
//!
//! These helpers enforce the *runtime* properties we need for spawn safety:
//! absolute paths (or a single PATH lookup name), no interior NULs, existing
//! regular files after canonicalize where required, and optional containment
//! under a trusted root or equality with a known helper beside another binary.
//! Paths stay as [`PathBuf`] / [`OsStr`] end-to-end — character allowlists do
//! not establish executable provenance and reject valid spaces / Unicode /
//! non-UTF-8 Unix paths.
//!
//! Static analysis: successful `Ok(PathBuf)` returns are modeled as
//! `command-injection` / `path-injection` barriers in
//! `.github/codeql/model-packs/bookclerk-rust`. Call sites may still add
//! `// codeql[rust/command-line-injection]` when a given CodeQL build does not
//! load that model pack.

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

/// Rebuild after validation so later sinks do not share the pre-check `PathBuf`.
fn path_after_validation(path: &Path) -> PathBuf {
    PathBuf::from(path.as_os_str().to_os_string())
}

/// Absolute path with no NUL, or a single PATH lookup name (`cargo`, `python3`).
///
/// Does not require the path to exist (useful for generated argv file operands).
/// Lexical `..` in an absolute spelling is allowed; callers that need a real
/// file should use [`require_spawn_executable`] (canonicalize).
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

/// Absolute existing regular file (or a single PATH name), with no interior NUL.
///
/// Absolute paths are canonicalized so symlink targets and lexical `..` resolve
/// to the file that will actually be executed. PATH lookup names are returned
/// as-is for the OS to resolve at spawn time.
///
/// # Errors
///
/// Returns [`SpawnPathError`] when validation fails, canonicalize fails, or an
/// absolute path is not a regular file.
pub fn require_spawn_executable(path: &Path) -> Result<PathBuf, SpawnPathError> {
    let path = require_absolute_or_name(path)?;
    if path.is_absolute() {
        let canon =
            std::fs::canonicalize(&path).map_err(|source| SpawnPathError::Canonicalize {
                path: path.clone(),
                source,
            })?;
        // codeql[rust/path-injection]
        if !canon.is_file() {
            return Err(SpawnPathError::NotFile(canon));
        }
        return Ok(path_after_validation(&canon));
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
/// env overrides still require [`require_spawn_executable`]. Operator-selectable
/// absolute overrides are an explicit trust decision (document/model that at the
/// call site) — filename characters alone never make an arbitrary executable safe.
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
    fn spawn_executable_accepts_space_in_filename() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let bin = dir.path().join("my helper");
        std::fs::write(&bin, b"x").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }
        let got = require_spawn_executable(&bin).expect("space ok");
        assert_eq!(got, std::fs::canonicalize(&bin).unwrap());
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
