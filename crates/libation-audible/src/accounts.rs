//! Account listing / import helpers.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AudibleError, Result};

/// Summary of a configured account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub account_id: String,
    pub marketplace: String,
    pub label: Option<String>,
    pub status: AccountStatus,
}

/// Token health.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Valid,
    ExpiringSoon,
    Expired,
    Unknown,
}

/// List accounts known to Libation / audible-rs (scaffold: empty or JSON import).
pub fn list_accounts_stub(_files_dir: &Path) -> Result<Vec<AccountInfo>> {
    // Real implementation reads audible-rs auth files + Libation DB.
    Ok(Vec::new())
}

/// Import Libation `AccountsSettings.json` (scaffold parses & validates shape).
pub fn import_libation_accounts_json(path: &Path) -> Result<Vec<AccountInfo>> {
    let text = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| AudibleError::Import(format!("invalid JSON: {err}")))?;

    // Libation AccountsSettings.json is typically an array or `{ Accounts: [...] }`.
    let accounts = if let Some(arr) = value.as_array() {
        arr.clone()
    } else if let Some(arr) = value.get("Accounts").and_then(|v| v.as_array()) {
        arr.clone()
    } else if let Some(arr) = value.get("accounts").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        return Err(AudibleError::Import(
            "expected AccountsSettings.json array or object with Accounts".into(),
        ));
    };

    let mut out = Vec::new();
    for (idx, acct) in accounts.iter().enumerate() {
        let account_id = acct
            .get("AccountId")
            .or_else(|| acct.get("account_id"))
            .or_else(|| acct.get("AccountName"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("imported-{idx}"));
        let marketplace = acct
            .get("Locale")
            .or_else(|| acct.get("marketplace"))
            .or_else(|| acct.get("CountryCode"))
            .and_then(|v| v.as_str())
            .unwrap_or("us")
            .to_string();
        let label = acct
            .get("AccountName")
            .or_else(|| acct.get("label"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        out.push(AccountInfo {
            account_id,
            marketplace,
            label,
            status: AccountStatus::Unknown,
        });
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn import_accounts_array() {
        let mut f = NamedTempFile::new().unwrap();
        write!(
            f,
            r#"[{{"AccountId":"abc","Locale":"us","AccountName":"Main"}}]"#
        )
        .unwrap();
        let accounts = import_libation_accounts_json(f.path()).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].account_id, "abc");
        assert_eq!(accounts[0].marketplace, "us");
    }
}
