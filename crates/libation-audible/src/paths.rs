//! Account file layout under `LIBATION_FILES_DIR`.
//!
//! | Path | Contents |
//! | --- | --- |
//! | `Accounts/<name>.auth` | Audible OAuth envelope (encrypted at rest) |
//! | `Accounts/<name>.wvd` | Widevine L3 CDM |
//! | `auth/<name>.auth` | Legacy location — still read for migration |

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Legacy directory for audible-rs `.auth` envelopes (`{files_dir}/auth/`).
#[must_use]
pub fn legacy_auth_dir(files_dir: &Path) -> PathBuf {
    files_dir.join("auth")
}

/// Alias retained for callers that still say `auth_dir` (legacy layout).
#[must_use]
pub fn auth_dir(files_dir: &Path) -> PathBuf {
    legacy_auth_dir(files_dir)
}

/// Directory for per-account auth + CDM artifacts (`{files_dir}/Accounts/`).
#[must_use]
pub fn accounts_dir(files_dir: &Path) -> PathBuf {
    files_dir.join("Accounts")
}

/// Canonical path for one account's auth file (`{files_dir}/Accounts/{name}.auth`).
#[must_use]
pub fn auth_file_for(files_dir: &Path, account_name: &str) -> PathBuf {
    accounts_dir(files_dir).join(format!("{}.auth", sanitize_name(account_name)))
}

/// Legacy path (`{files_dir}/auth/{name}.auth`).
#[must_use]
pub fn legacy_auth_file_for(files_dir: &Path, account_name: &str) -> PathBuf {
    legacy_auth_dir(files_dir).join(format!("{}.auth", sanitize_name(account_name)))
}

/// Path for one account's Widevine L3 CDM (`{files_dir}/Accounts/{name}.wvd`).
#[must_use]
pub fn widevine_cdm_file_for(files_dir: &Path, account_name: &str) -> PathBuf {
    accounts_dir(files_dir).join(format!("{}.wvd", sanitize_name(account_name)))
}

/// Ensure `Accounts/` exists with restrictive permissions.
pub fn ensure_accounts_dir(files_dir: &Path) -> std::io::Result<PathBuf> {
    let dir = accounts_dir(files_dir);
    std::fs::create_dir_all(&dir)?;
    let _ = crate::secret::harden_secret_path(&dir);
    Ok(dir)
}

fn collect_auth_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("auth") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// List `*.auth` files: canonical `Accounts/` wins over legacy `auth/` for the same stem.
pub fn list_auth_files(files_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut by_stem: BTreeMap<String, PathBuf> = BTreeMap::new();
    for path in collect_auth_files(&legacy_auth_dir(files_dir))? {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        by_stem.insert(stem, path);
    }
    for path in collect_auth_files(&accounts_dir(files_dir))? {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        by_stem.insert(stem, path);
    }
    Ok(by_stem.into_values().collect())
}

/// Sanitize an account name for use as a filename stem.
#[must_use]
pub fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | ' ' => out.push('_'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let trimmed = out.trim().trim_matches('.');
    if trimmed.is_empty() {
        "account".into()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_name_and_paths() {
        assert_eq!(sanitize_name("Main US"), "Main_US");
        assert_eq!(
            auth_file_for(Path::new("/data"), "us"),
            PathBuf::from("/data/Accounts/us.auth")
        );
        assert_eq!(
            legacy_auth_file_for(Path::new("/data"), "us"),
            PathBuf::from("/data/auth/us.auth")
        );
        assert_eq!(
            widevine_cdm_file_for(Path::new("/data"), "us"),
            PathBuf::from("/data/Accounts/us.wvd")
        );
    }

    #[test]
    fn list_prefers_accounts_over_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let accounts = accounts_dir(dir.path());
        let legacy = legacy_auth_dir(dir.path());
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(accounts.join("alice.auth"), b"{}").unwrap();
        std::fs::write(legacy.join("alice.auth"), b"{}").unwrap();
        std::fs::write(legacy.join("bob.auth"), b"{}").unwrap();
        let list = list_auth_files(dir.path()).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|p| p.ends_with("Accounts/alice.auth")));
        assert!(list.iter().any(|p| p.ends_with("auth/bob.auth")));
        assert!(!list.iter().any(|p| p.ends_with("auth/alice.auth")));
    }
}
