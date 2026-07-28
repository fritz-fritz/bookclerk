//! Local SQLite via SeaORM `ProxyDatabaseTrait` wrapping rusqlite.
//!
//! SeaORM 2.0's `sqlx-sqlite` driver does not currently compile against
//! `sea-query` 1.0.x (`Value` no longer boxes payloads). The proxy path keeps
//! a single `libsqlite3` (shared with audible-rs) and one ORM API for SQLite + D1.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;
use sea_orm::{DbErr, ProxyDatabaseTrait, ProxyExecResult, ProxyRow, Statement, Value};

/// SeaORM proxy over a shared rusqlite connection.
#[derive(Debug)]
pub struct SqliteProxy {
    conn: Mutex<Connection>,
}

impl SqliteProxy {
    #[must_use]
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }
}

#[async_trait]
impl ProxyDatabaseTrait for SqliteProxy {
    async fn query(&self, statement: Statement) -> std::result::Result<Vec<ProxyRow>, DbErr> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| DbErr::Custom(format!("sqlite mutex poisoned: {e}")))?;
        let mut stmt = conn
            .prepare(&statement.sql)
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        let binds = statement_binds(&statement);
        let names: Vec<String> = (0..stmt.column_count())
            .map(|i| stmt.column_name(i).unwrap_or("").to_string())
            .collect();
        let mut rows = stmt
            .query(rusqlite::params_from_iter(binds.iter()))
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| DbErr::Custom(e.to_string()))? {
            let mut values = BTreeMap::new();
            for (i, name) in names.iter().enumerate() {
                let v: rusqlite::types::Value =
                    row.get(i).map_err(|e| DbErr::Custom(e.to_string()))?;
                values.insert(name.clone(), rusqlite_to_sea(v));
            }
            out.push(ProxyRow { values });
        }
        Ok(out)
    }

    async fn execute(&self, statement: Statement) -> std::result::Result<ProxyExecResult, DbErr> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| DbErr::Custom(format!("sqlite mutex poisoned: {e}")))?;
        let binds = statement_binds(&statement);
        conn.execute(&statement.sql, rusqlite::params_from_iter(binds.iter()))
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        Ok(ProxyExecResult {
            last_insert_id: conn.last_insert_rowid() as u64,
            rows_affected: conn.changes(),
        })
    }

    async fn ping(&self) -> std::result::Result<(), DbErr> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| DbErr::Custom(format!("sqlite mutex poisoned: {e}")))?;
        conn.prepare("SELECT 1")
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        Ok(())
    }
}

fn statement_binds(statement: &Statement) -> Vec<rusqlite::types::Value> {
    match &statement.values {
        Some(values) => values.0.iter().map(sea_to_rusqlite).collect(),
        None => Vec::new(),
    }
}

fn sea_to_rusqlite(v: &Value) -> rusqlite::types::Value {
    match v {
        Value::Bool(Some(b)) => rusqlite::types::Value::Integer(i64::from(*b)),
        Value::TinyInt(Some(n)) => rusqlite::types::Value::Integer(i64::from(*n)),
        Value::SmallInt(Some(n)) => rusqlite::types::Value::Integer(i64::from(*n)),
        Value::Int(Some(n)) => rusqlite::types::Value::Integer(i64::from(*n)),
        Value::BigInt(Some(n)) => rusqlite::types::Value::Integer(*n),
        Value::Float(Some(n)) => rusqlite::types::Value::Real(f64::from(*n)),
        Value::Double(Some(n)) => rusqlite::types::Value::Real(*n),
        Value::String(Some(s)) => rusqlite::types::Value::Text(s.to_string()),
        Value::Bytes(Some(b)) => rusqlite::types::Value::Blob(b.to_vec()),
        Value::ChronoDateTimeUtc(Some(dt)) => rusqlite::types::Value::Text(dt.to_rfc3339()),
        Value::ChronoDateTime(Some(dt)) => rusqlite::types::Value::Text(dt.and_utc().to_rfc3339()),
        _ => rusqlite::types::Value::Null,
    }
}

fn rusqlite_to_sea(v: rusqlite::types::Value) -> Value {
    match v {
        rusqlite::types::Value::Null => Value::String(None),
        rusqlite::types::Value::Integer(n) => Value::BigInt(Some(n)),
        rusqlite::types::Value::Real(n) => Value::Double(Some(n)),
        rusqlite::types::Value::Text(s) => Value::String(Some(s)),
        rusqlite::types::Value::Blob(b) => Value::Bytes(Some(b)),
    }
}
