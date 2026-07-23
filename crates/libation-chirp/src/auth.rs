//! Chirp auth file persistence under `Accounts/*.chirp.auth`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ChirpError, Result};

/// On-disk auth envelope for one Chirp account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChirpAuthFile {
    /// JWT from GraphQL `signIn` (`user.token`).
    pub access_token: String,
    /// Longer-lived web JWT when present (`user.webToken`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_token: Option<String>,
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default = "default_marketplace")]
    pub marketplace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

fn default_marketplace() -> String {
    String::from("us")
}

impl ChirpAuthFile {
    /// Stable account id: prefer Chirp user id, else email.
    #[must_use]
    pub fn account_id(&self) -> &str {
        self.user_id.as_deref().unwrap_or(self.email.as_str())
    }
}

#[must_use]
pub fn accounts_dir(files_dir: &Path) -> PathBuf {
    files_dir.join("Accounts")
}

pub fn ensure_accounts_dir(files_dir: &Path) -> std::io::Result<PathBuf> {
    let dir = accounts_dir(files_dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[must_use]
pub fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | ' ' | '@' => out.push('_'),
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

#[must_use]
pub fn auth_stem(label: Option<&str>, email: &str) -> String {
    if let Some(label) = label.map(str::trim).filter(|s| !s.is_empty()) {
        return sanitize_name(label);
    }
    let stem = email.split('@').next().unwrap_or(email);
    sanitize_name(stem)
}

#[must_use]
pub fn auth_file_for(files_dir: &Path, stem: &str) -> PathBuf {
    accounts_dir(files_dir).join(format!("{}.chirp.auth", sanitize_name(stem)))
}

#[must_use]
pub fn auth_file_for_account(files_dir: &Path, label: Option<&str>, email: &str) -> PathBuf {
    auth_file_for(files_dir, &auth_stem(label, email))
}

pub fn list_auth_files(files_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let dir = accounts_dir(files_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if name.ends_with(".chirp.auth") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

pub fn save_auth(path: &Path, auth: &ChirpAuthFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(auth)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_auth(path: &Path) -> Result<ChirpAuthFile> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ChirpError::AccountNotFound(path.display().to_string())
        } else {
            ChirpError::Io(e)
        }
    })?;
    let auth: ChirpAuthFile = serde_json::from_str(&raw)?;
    if auth.access_token.is_empty() {
        return Err(ChirpError::auth("auth file missing access_token"));
    }
    if auth.email.is_empty() {
        return Err(ChirpError::auth("auth file missing email"));
    }
    Ok(auth)
}

pub fn find_auth_file(files_dir: &Path, account_id: &str) -> Result<PathBuf> {
    let needle = account_id.trim();
    if needle.is_empty() {
        return Err(ChirpError::AccountNotFound("empty account id".into()));
    }

    let by_stem = auth_file_for(files_dir, needle);
    if by_stem.is_file() {
        return Ok(by_stem);
    }

    for path in list_auth_files(files_dir)? {
        let auth = match load_auth(&path) {
            Ok(auth) => auth,
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "skipping unreadable Chirp auth file during account lookup"
                );
                continue;
            }
        };
        if auth.email.eq_ignore_ascii_case(needle)
            || auth.account_id().eq_ignore_ascii_case(needle)
            || auth
                .label
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case(needle))
        {
            return Ok(path);
        }
        let stem = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".chirp.auth"))
            .unwrap_or("");
        if stem.eq_ignore_ascii_case(needle) {
            return Ok(path);
        }
    }

    Err(ChirpError::AccountNotFound(needle.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_auth_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = auth_file_for_account(dir.path(), None, "bob@chirp.example");
        let auth = ChirpAuthFile {
            access_token: "tok".into(),
            web_token: Some("web".into()),
            email: "bob@chirp.example".into(),
            user_id: Some("42".into()),
            marketplace: "us".into(),
            label: None,
        };
        save_auth(&path, &auth).unwrap();
        let loaded = load_auth(&path).unwrap();
        assert_eq!(loaded.account_id(), "42");
        assert_eq!(list_auth_files(dir.path()).unwrap().len(), 1);
    }
}
