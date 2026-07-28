//! Cloudflare D1 HTTP API proxy for SeaORM.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use bookclerk_config::Config;
use sea_orm::{DbErr, ProxyDatabaseTrait, ProxyExecResult, ProxyRow, Statement, Value};
use serde_json::{json, Value as JsonValue};

use crate::error::{LibraryError, Result};

/// Resolve the D1 API token from env or `Accounts/*.d1.auth`.
pub fn resolve_d1_api_token(config: &Config) -> Result<String> {
    if let Ok(v) = std::env::var("BOOKCLERK_D1_API_TOKEN") {
        let t = v.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Ok(v) = std::env::var("CLOUDFLARE_API_TOKEN") {
        let t = v.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Some(path) = config.resolved_d1_credentials_path() {
        let raw = std::fs::read_to_string(&path)?;
        let value: JsonValue = serde_json::from_str(&raw).map_err(|e| {
            LibraryError::Other(anyhow::anyhow!(
                "invalid D1 credentials JSON at {}: {e}",
                path.display()
            ))
        })?;
        if let Some(token) = value.get("api_token").and_then(|v| v.as_str()) {
            let t = token.trim();
            if !t.is_empty() {
                return Ok(t.to_string());
            }
        }
        return Err(LibraryError::Other(anyhow::anyhow!(
            "D1 credentials file {} missing non-empty api_token",
            path.display()
        )));
    }
    Err(LibraryError::Other(anyhow::anyhow!(
        "D1 API token not configured — set BOOKCLERK_D1_API_TOKEN / CLOUDFLARE_API_TOKEN \
         or Accounts/default.d1.auth (see docs/database.md)"
    )))
}

/// SeaORM proxy that executes statements against Cloudflare D1's HTTP API.
#[derive(Debug)]
pub struct D1Proxy {
    api_base: String,
    account_id: String,
    database_id: String,
    api_token: String,
    client: reqwest::Client,
    /// D1 does not support real transactions; track nesting for SeaORM begin/commit.
    txn_depth: Mutex<u32>,
}

impl D1Proxy {
    #[must_use]
    pub fn new(
        api_base: String,
        account_id: String,
        database_id: String,
        api_token: String,
    ) -> Self {
        Self {
            api_base: api_base.trim_end_matches('/').to_string(),
            account_id,
            database_id,
            api_token,
            client: reqwest::Client::new(),
            txn_depth: Mutex::new(0),
        }
    }

    fn query_url(&self) -> String {
        format!(
            "{}/accounts/{}/d1/database/{}/query",
            self.api_base, self.account_id, self.database_id
        )
    }

    async fn run_sql(
        &self,
        sql: &str,
        params: Vec<JsonValue>,
    ) -> std::result::Result<JsonValue, DbErr> {
        let body = json!({
            "sql": sql,
            "params": params,
        });
        let response = self
            .client
            .post(self.query_url())
            .bearer_auth(&self.api_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| DbErr::Custom(format!("D1 HTTP error: {e}")))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| DbErr::Custom(format!("D1 read body: {e}")))?;
        if !status.is_success() {
            return Err(DbErr::Custom(format!(
                "D1 HTTP {status}: {}",
                truncate(&text, 500)
            )));
        }
        let value: JsonValue = serde_json::from_str(&text).map_err(|e| {
            DbErr::Custom(format!("D1 JSON parse: {e}; body={}", truncate(&text, 200)))
        })?;
        if value.get("success").and_then(|v| v.as_bool()) != Some(true) {
            return Err(DbErr::Custom(format!(
                "D1 API unsuccessful: {}",
                truncate(&text, 500)
            )));
        }
        Ok(value)
    }
}

#[async_trait]
impl ProxyDatabaseTrait for D1Proxy {
    async fn query(&self, statement: Statement) -> std::result::Result<Vec<ProxyRow>, DbErr> {
        let params = statement_json_params(&statement);
        let value = self.run_sql(&statement.sql, params).await?;
        let results = value
            .get("result")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("results"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut rows = Vec::with_capacity(results.len());
        for row in results {
            let Some(obj) = row.as_object() else {
                continue;
            };
            let mut values = BTreeMap::new();
            for (k, v) in obj {
                values.insert(k.clone(), json_to_sea_value(v, k));
            }
            rows.push(ProxyRow { values });
        }
        Ok(rows)
    }

    async fn execute(&self, statement: Statement) -> std::result::Result<ProxyExecResult, DbErr> {
        let params = statement_json_params(&statement);
        let value = self.run_sql(&statement.sql, params).await?;
        let meta = value
            .get("result")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("meta"));
        let last_insert_id = meta
            .and_then(|m| m.get("last_row_id"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let rows_affected = meta
            .and_then(|m| m.get("changes"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Ok(ProxyExecResult {
            last_insert_id,
            rows_affected,
        })
    }

    async fn begin(&self) {
        if let Ok(mut depth) = self.txn_depth.lock() {
            *depth = depth.saturating_add(1);
        }
        tracing::debug!("D1 proxy begin (no-op; D1 lacks interactive transactions)");
    }

    async fn commit(&self) {
        if let Ok(mut depth) = self.txn_depth.lock() {
            *depth = depth.saturating_sub(1);
        }
    }

    async fn rollback(&self) {
        if let Ok(mut depth) = self.txn_depth.lock() {
            *depth = depth.saturating_sub(1);
        }
        tracing::warn!("D1 proxy rollback requested (no-op; prior statements already applied)");
    }

    async fn ping(&self) -> std::result::Result<(), DbErr> {
        let _ = self.run_sql("SELECT 1 AS ok;", Vec::new()).await?;
        Ok(())
    }
}

fn statement_json_params(statement: &Statement) -> Vec<JsonValue> {
    match &statement.values {
        Some(values) => values.0.iter().map(sea_value_to_json).collect(),
        None => Vec::new(),
    }
}

fn sea_value_to_json(v: &Value) -> JsonValue {
    match v {
        Value::Bool(Some(b)) => JsonValue::Bool(*b),
        Value::TinyInt(Some(n)) => JsonValue::from(*n),
        Value::SmallInt(Some(n)) => JsonValue::from(*n),
        Value::Int(Some(n)) => JsonValue::from(*n),
        Value::BigInt(Some(n)) => JsonValue::from(*n),
        Value::Float(Some(n)) => JsonValue::from(f64::from(*n)),
        Value::Double(Some(n)) => JsonValue::from(*n),
        Value::String(Some(s)) => JsonValue::String(s.to_string()),
        Value::Bytes(Some(b)) => JsonValue::String(String::from_utf8_lossy(b).into_owned()),
        Value::ChronoDateTimeUtc(Some(dt)) => JsonValue::String(dt.to_rfc3339()),
        Value::ChronoDateTime(Some(dt)) => JsonValue::String(dt.and_utc().to_rfc3339()),
        _ => JsonValue::Null,
    }
}

fn json_to_sea_value(v: &JsonValue, column: &str) -> Value {
    match v {
        JsonValue::Null => crate::db::typed_null(None, column),
        JsonValue::Bool(b) => Value::Bool(Some(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::BigInt(Some(i))
            } else if let Some(f) = n.as_f64() {
                Value::Double(Some(f))
            } else {
                Value::String(Some(n.to_string()))
            }
        }
        JsonValue::String(s) => Value::String(Some(s.clone())),
        other => Value::String(Some(other.to_string())),
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
