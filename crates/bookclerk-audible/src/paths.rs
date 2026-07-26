//! Account file layout under `BOOKCLERK_FILES_DIR`.
//!
//! | Path | Contents |
//! | --- | --- |
//! | `Accounts/<name>.audible.auth` | Audible OAuth envelope (encrypted or plaintext) |
//! | `Accounts/<name>.auth` | Legacy Audible envelope (still read; renamed on list) |
//! | `Accounts/<name>.wvd` | Widevine L3 CDM |

use std::path::{Path, PathBuf};

/// On-disk auth suffix for Audible accounts (aligned with `.libro.auth`, etc.).
pub const AUTH_SUFFIX: &str = ".audible.auth";

/// Pre-alignment Audible suffix (`Accounts/<stem>.auth`).
pub const LEGACY_AUTH_SUFFIX: &str = ".auth";

/// Directory for per-account auth + CDM artifacts (`{files_dir}/Accounts/`).
#[must_use]
pub fn accounts_dir(files_dir: &Path) -> PathBuf {
    bookclerk_source::accounts_dir(files_dir)
}

/// Canonical path for one account's auth file
/// (`{files_dir}/Accounts/{name}.audible.auth`).
#[must_use]
pub fn auth_file_for(files_dir: &Path, account_name: &str) -> PathBuf {
    bookclerk_source::auth_file_for(files_dir, account_name, AUTH_SUFFIX)
}

/// Legacy path `Accounts/{name}.auth` (pre-`.audible.auth` alignment).
#[must_use]
pub fn legacy_auth_file_for(files_dir: &Path, account_name: &str) -> PathBuf {
    bookclerk_source::auth_file_for(files_dir, account_name, LEGACY_AUTH_SUFFIX)
}

/// Path for one account's Widevine L3 CDM (`{files_dir}/Accounts/{name}.wvd`).
#[must_use]
pub fn widevine_cdm_file_for(files_dir: &Path, account_name: &str) -> PathBuf {
    accounts_dir(files_dir).join(format!("{}.wvd", sanitize_name(account_name)))
}

/// Ensure `Accounts/` exists with restrictive permissions.
pub fn ensure_accounts_dir(files_dir: &Path) -> std::io::Result<PathBuf> {
    let dir = bookclerk_source::ensure_accounts_dir(files_dir)?;
    let _ = crate::secret::harden_secret_path(&dir);
    Ok(dir)
}

/// Filename stem from an Audible auth path (`alice.audible.auth` / legacy `alice.auth`).
#[must_use]
pub fn auth_stem_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    auth_stem_from_name(name).map(str::to_string)
}

/// Filename stem from an Audible auth filename.
#[must_use]
pub fn auth_stem_from_name(name: &str) -> Option<&str> {
    if let Some(stem) = name.strip_suffix(AUTH_SUFFIX) {
        return Some(stem);
    }
    // Other sources also end in `.auth`; never treat them as Audible.
    if name.ends_with(".libro.auth")
        || name.ends_with(".ga.auth")
        || name.ends_with(".chirp.auth")
        || name.ends_with(".s3.auth")
    {
        return None;
    }
    name.strip_suffix(LEGACY_AUTH_SUFFIX)
}

/// List Audible auth files under `Accounts/`.
///
/// Prefers `*.audible.auth`. Legacy bare `*.auth` files are still discovered and
/// renamed to `*.audible.auth` when the destination does not already exist.
pub fn list_auth_files(files_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let _ = migrate_legacy_auth_files(files_dir);
    let mut out = bookclerk_source::list_auth_files(files_dir, AUTH_SUFFIX)?;
    // Include leftover legacy files when rename was skipped/failed and no
    // canonical sibling exists (avoids dropping accounts on conflict/IO error).
    for path in list_unmigrated_legacy_auth_files(files_dir)? {
        out.push(path);
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn list_unmigrated_legacy_auth_files(files_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let dir = accounts_dir(files_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name.ends_with(AUTH_SUFFIX) {
            continue;
        }
        let Some(stem) = auth_stem_from_name(name) else {
            continue;
        };
        if auth_file_for(files_dir, stem).exists() {
            continue;
        }
        out.push(path);
    }
    out.sort();
    Ok(out)
}

/// Rename legacy `Accounts/<stem>.auth` → `Accounts/<stem>.audible.auth` when safe.
pub fn migrate_legacy_auth_files(files_dir: &Path) -> std::io::Result<usize> {
    let dir = accounts_dir(files_dir);
    if !dir.exists() {
        return Ok(0);
    }
    let mut migrated = 0usize;
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name.ends_with(AUTH_SUFFIX) {
            continue;
        }
        let Some(stem) = auth_stem_from_name(name) else {
            continue;
        };
        let dest = auth_file_for(files_dir, stem);
        if dest.exists() {
            tracing::warn!(
                legacy = %path.display(),
                canonical = %dest.display(),
                "skipping legacy Audible auth rename; canonical file already exists"
            );
            continue;
        }
        std::fs::rename(&path, &dest)?;
        tracing::info!(
            from = %path.display(),
            to = %dest.display(),
            "renamed legacy Audible auth file to .audible.auth"
        );
        migrated += 1;
    }
    Ok(migrated)
}

/// Sanitize an account name for use as a filename stem.
#[must_use]
pub fn sanitize_name(name: &str) -> String {
    bookclerk_source::sanitize_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_name_and_paths() {
        assert_eq!(sanitize_name("Main US"), "Main_US");
        assert_eq!(
            auth_file_for(Path::new("/data"), "us"),
            PathBuf::from("/data/Accounts/us.audible.auth")
        );
        assert_eq!(
            widevine_cdm_file_for(Path::new("/data"), "us"),
            PathBuf::from("/data/Accounts/us.wvd")
        );
    }

    #[test]
    fn stem_from_canonical_and_legacy_names() {
        assert_eq!(auth_stem_from_name("alice.audible.auth"), Some("alice"));
        assert_eq!(auth_stem_from_name("alice.auth"), Some("alice"));
        assert_eq!(auth_stem_from_name("alice.libro.auth"), None);
        assert_eq!(auth_stem_from_name("alice.s3.auth"), None);
    }

    #[test]
    fn list_only_audible_auth_files_and_migrates_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let accounts = accounts_dir(dir.path());
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(accounts.join("alice.auth"), b"{}").unwrap();
        // Other sources share Accounts/ but must not be treated as Audible.
        std::fs::write(accounts.join("alice.libro.auth"), b"{}").unwrap();
        std::fs::write(accounts.join("alice.ga.auth"), b"{}").unwrap();
        std::fs::write(accounts.join("alice.chirp.auth"), b"{}").unwrap();
        std::fs::write(accounts.join("default.s3.auth"), b"{}").unwrap();
        // Stray non-auth files must be ignored.
        std::fs::write(accounts.join("notes.txt"), b"x").unwrap();
        // Stray legacy dir must be ignored.
        let legacy = dir.path().join("auth");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("bob.auth"), b"{}").unwrap();

        let list = list_auth_files(dir.path()).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].ends_with("Accounts/alice.audible.auth"));
        assert!(!accounts.join("alice.auth").exists());
        assert!(accounts.join("alice.audible.auth").is_file());
    }

    #[test]
    fn list_prefers_existing_canonical_over_legacy_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let accounts = accounts_dir(dir.path());
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(accounts.join("alice.audible.auth"), b"new").unwrap();
        std::fs::write(accounts.join("alice.auth"), b"old").unwrap();
        let list = list_auth_files(dir.path()).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].ends_with("Accounts/alice.audible.auth"));
        // Legacy left in place when canonical already exists.
        assert!(accounts.join("alice.auth").is_file());
    }
}
