//! Database plugin Workers RPC DTOs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// SQL + bind parameters crossing the host↔database-guest boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StatementDto {
    /// Sql.
    pub sql: String,
    /// Values.
    #[serde(default)]
    pub values: Vec<JsonValue>,
}

/// One result row from `dbQuery`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRowDto {
    /// Values.
    pub values: BTreeMap<String, JsonValue>,
}

/// Result of `dbQuery`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueryResultDto {
    /// Rows.
    pub rows: Vec<ProxyRowDto>,
}

/// Result of `dbExecute`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecResultDto {
    /// Last insert Identifier.
    pub last_insert_id: u64,
    /// Rows affected.
    pub rows_affected: u64,
}

/// Params for `dbConnect` (tagged by backend).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum DbConnectParams {
    /// Sqlite variant.
    #[serde(rename_all = "camelCase")]
    Sqlite {
        /// Plugin data dir.
        plugin_data_dir: String,
        /// Host fallback path when no fd side channel is wired.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sqlite_path: Option<String>,
    },
    /// D1 variant.
    #[serde(rename_all = "camelCase")]
    D1 {
        /// Plugin data dir.
        plugin_data_dir: String,
        /// Account Identifier.
        account_id: String,
        /// Database Identifier.
        database_id: String,
        /// API base.
        api_base: String,
        /// API token.
        api_token: String,
    },
    /// Postgres variant.
    #[serde(rename_all = "camelCase")]
    Postgres {
        /// Plugin data dir.
        plugin_data_dir: String,
        /// URL.
        url: String,
    },
}

/// Result of a successful `dbConnect`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbConnectResult {
    /// SeaORM dialect the host should use for the RPC proxy (`sqlite` or `postgres`).
    pub dialect: String,
}

impl DbConnectResult {
    /// Sqlite.
    #[must_use]
    pub fn sqlite() -> Self {
        Self {
            dialect: String::from("sqlite"),
        }
    }

    /// Postgres.
    #[must_use]
    pub fn postgres() -> Self {
        Self {
            dialect: String::from("postgres"),
        }
    }
}
