//! Auth and account file layout under `LIBATION_FILES_DIR`.

use std::path::{Path, PathBuf};

/// Directory for audible-rs `.auth` envelopes (`{files_dir}/auth/`).
#[must_use]
pub fn auth_dir(files_dir: &Path) -> PathBuf {
    files_dir.join("auth")
}

/// Directory for per-account artifacts such as Widevine CDMs (`{files_dir}/Accounts/`).
#[must_use]
pub fn accounts_dir(files_dir: &Path) -> PathBuf {
    files_dir.join("Accounts")
}

/// Path for one account's auth file (`{files_dir}/auth/{name}.auth`).
#[must_use]
pub fn auth_file_for(files_dir: &Path, account_name: &str) -> PathBuf {
    auth_dir(files_dir).join(format!("{}.auth", sanitize_name(account_name)))
}

/// Path for one account's Widevine L3 CDM (`{files_dir}/Accounts/{name}.wvd`).
#[must_use]
pub fn widevine_cdm_file_for(files_dir: &Path, account_name: &str) -> PathBuf {
    accounts_dir(files_dir).join(format!("{}.wvd", sanitize_name(account_name)))
}

/// List `*.auth` files in the auth directory.
pub fn list_auth_files(files_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let dir = auth_dir(files_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("auth") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
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
    fn sanitizes_name() {
        assert_eq!(sanitize_name("Main US"), "Main_US");
        assert_eq!(
            auth_file_for(Path::new("/data"), "us"),
            PathBuf::from("/data/auth/us.auth")
        );
        assert_eq!(
            widevine_cdm_file_for(Path::new("/data"), "us"),
            PathBuf::from("/data/Accounts/us.wvd")
        );
    }
}
