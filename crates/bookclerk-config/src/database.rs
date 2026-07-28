//! Database backend plugins (`[database]`, `[database.sqlite]`, `[database.d1]`).
//!
//! Exactly one backend is active (`plugin`). Default is local SQLite.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};

fn default_plugin() -> String {
    String::from("sqlite")
}

fn default_true() -> bool {
    true
}

fn default_d1_api_base() -> String {
    String::from("https://api.cloudflare.com/client/v4")
}

/// Which first-party database plugin is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DatabasePluginKind {
    #[default]
    Sqlite,
    D1,
}

impl DatabasePluginKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::D1 => "d1",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sqlite" | "local" => Some(Self::Sqlite),
            "d1" | "cloudflare-d1" | "cloudflare_d1" => Some(Self::D1),
            _ => None,
        }
    }
}

/// Top-level `[database]` — selects the active backend plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DatabaseConfig {
    /// Active plugin id (`sqlite` or `d1`).
    pub plugin: String,
    pub sqlite: DatabaseSqliteConfig,
    pub d1: DatabaseD1Config,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            plugin: default_plugin(),
            sqlite: DatabaseSqliteConfig::default(),
            d1: DatabaseD1Config::default(),
        }
    }
}

impl DatabaseConfig {
    /// Parsed active plugin, or error if unknown.
    pub fn active_plugin(&self) -> Result<DatabasePluginKind> {
        DatabasePluginKind::parse(&self.plugin).ok_or_else(|| {
            ConfigError::Invalid(format!(
                "unknown [database].plugin `{}` (expected sqlite or d1)",
                self.plugin
            ))
        })
    }

    /// Resolve the SQLite file path (relative paths join `files_dir`).
    #[must_use]
    pub fn sqlite_path(&self, files_dir: &std::path::Path) -> PathBuf {
        let raw = self
            .sqlite
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from("library.db"));
        if raw.is_absolute() {
            raw
        } else {
            files_dir.join(raw)
        }
    }

    /// Soft validation for the selected plugin.
    pub fn validate(&self) -> Result<()> {
        match self.active_plugin()? {
            DatabasePluginKind::Sqlite => Ok(()),
            DatabasePluginKind::D1 => {
                if self.d1.account_id.trim().is_empty() {
                    return Err(ConfigError::Invalid(
                        "[database.d1].account_id is required when plugin = \"d1\"".into(),
                    ));
                }
                if self.d1.database_id.trim().is_empty() {
                    return Err(ConfigError::Invalid(
                        "[database.d1].database_id is required when plugin = \"d1\"".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Local SQLite destination (`[database.sqlite]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DatabaseSqliteConfig {
    /// Kept for symmetry with other plugins; ignored when another plugin is active.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// DB file path (default `library.db` under the files dir).
    pub path: Option<PathBuf>,
}

impl Default for DatabaseSqliteConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: None,
        }
    }
}

/// Cloudflare D1 backend (`[database.d1]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DatabaseD1Config {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub account_id: String,
    pub database_id: String,
    /// Path to JSON credentials (`Accounts/*.d1.auth`), relative to files dir or absolute.
    pub credentials_file: Option<PathBuf>,
    /// Cloudflare API base (default `https://api.cloudflare.com/client/v4`).
    #[serde(default = "default_d1_api_base")]
    pub api_base: String,
}

impl Default for DatabaseD1Config {
    fn default() -> Self {
        Self {
            enabled: true,
            account_id: String::new(),
            database_id: String::new(),
            credentials_file: None,
            api_base: default_d1_api_base(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_sqlite() {
        let db = DatabaseConfig::default();
        assert_eq!(db.active_plugin().unwrap(), DatabasePluginKind::Sqlite);
        assert!(db.validate().is_ok());
    }

    #[test]
    fn d1_requires_ids() {
        let mut db = DatabaseConfig {
            plugin: "d1".into(),
            ..Default::default()
        };
        assert!(db.validate().is_err());
        db.d1.account_id = "acct".into();
        db.d1.database_id = "db".into();
        assert!(db.validate().is_ok());
    }
}
