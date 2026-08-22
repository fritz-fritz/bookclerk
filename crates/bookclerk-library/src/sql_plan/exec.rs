//! Run a generic [`DbAtomicPlan`] on a SeaORM connection (one native transaction).

use std::time::Instant;

use bookclerk_plugin_abi::{DbAtomicPlan, DbAtomicResult, DbAtomicTiming, DbPlanStatementKind};
use sea_orm::{
    from_query_result_to_proxy_row, ConnectionTrait, DatabaseConnection, Statement,
    TransactionTrait, Value,
};
use serde_json::Value as JsonValue;

use super::interpret::{interpret_plan, PlanStmtResult};
use crate::error::{LibraryError, Result};

/// Executes `plan` as one transaction and interprets receipt/outcome rows.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when a statement fails. Application statuses
/// are returned as [`DbAtomicResult`], not errors.
pub async fn execute_plan_on(
    db: &DatabaseConnection,
    plan: &DbAtomicPlan,
    expected_hash: &str,
    operation_id: &str,
    timing_source: &str,
) -> Result<DbAtomicResult> {
    let started = Instant::now();
    let txn = db.begin().await.map_err(LibraryError::Orm)?;
    if crate::is_txn_broken() {
        let _ = txn.rollback().await;
        let fault = crate::take_txn_fault().unwrap_or_else(|| "database begin failed".into());
        return Err(LibraryError::Orm(sea_orm::DbErr::Custom(fault)));
    }
    let sql_started = Instant::now();
    let mut results = Vec::with_capacity(plan.statements.len());
    let backend = txn.get_database_backend();
    for stmt in &plan.statements {
        let values: Vec<Value> = stmt.binds.iter().map(json_to_sea).collect();
        let sea_stmt = Statement::from_sql_and_values(backend, &stmt.sql, values);
        let stmt_result = match stmt.kind {
            DbPlanStatementKind::Query => {
                let rows = match txn.query_all_raw(sea_stmt).await {
                    Ok(rows) => rows,
                    Err(err) => {
                        let _ = txn.rollback().await;
                        let _ = crate::take_txn_fault();
                        return Err(LibraryError::Orm(err));
                    }
                };
                let json_rows = rows
                    .into_iter()
                    .map(|row| {
                        let proxy = from_query_result_to_proxy_row(&row);
                        let mut map = serde_json::Map::new();
                        for (name, value) in proxy.values {
                            map.insert(name, sea_to_json(&value));
                        }
                        JsonValue::Object(map)
                    })
                    .collect();
                PlanStmtResult { rows: json_rows }
            }
            DbPlanStatementKind::Execute => {
                if let Err(err) = txn.execute_raw(sea_stmt).await {
                    let _ = txn.rollback().await;
                    let _ = crate::take_txn_fault();
                    return Err(LibraryError::Orm(err));
                }
                PlanStmtResult { rows: Vec::new() }
            }
        };
        results.push(stmt_result);
    }
    if crate::consume_commit_injection() {
        let _ = txn.rollback().await;
        let _ = crate::take_txn_fault();
        return Err(LibraryError::Orm(sea_orm::DbErr::Custom(
            "database commit failed: injected commit failure".into(),
        )));
    }
    txn.commit().await.map_err(|err| {
        let _ = crate::take_txn_fault();
        LibraryError::Orm(err)
    })?;
    if let Some(fault) = crate::take_txn_fault() {
        return Err(LibraryError::Orm(sea_orm::DbErr::Custom(fault)));
    }
    let db_execution_us = u64::try_from(sql_started.elapsed().as_micros()).unwrap_or(u64::MAX);
    let mut result = interpret_plan(plan, &results, expected_hash);
    result.operation_id = operation_id.to_string();
    result.timing = Some(DbAtomicTiming {
        attempt_elapsed_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        db_execution_us: Some(db_execution_us),
        db_timing_source: Some(timing_source.to_string()),
    });
    Ok(result)
}

/// Maps a JSON bind onto a SeaORM [`Value`], decoding `b64:` strings as blobs.
fn json_to_sea(v: &JsonValue) -> Value {
    if let Some(kind) = bookclerk_plugin_abi::sea_null_kind(v) {
        return match kind {
            "Bytes" => Value::Bytes(None),
            "BigInt" | "Int" | "TinyInt" | "SmallInt" | "TinyUnsigned" | "SmallUnsigned"
            | "Unsigned" | "BigUnsigned" => Value::BigInt(None),
            "Bool" => Value::Bool(None),
            "Double" | "Float" => Value::Double(None),
            _ => Value::String(None),
        };
    }
    match v {
        JsonValue::Null => Value::String(None),
        JsonValue::Bool(b) => Value::Bool(Some(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::BigInt(Some(i))
            } else if let Some(u) = n.as_u64() {
                Value::BigInt(Some(i64::try_from(u).unwrap_or(i64::MAX)))
            } else {
                Value::Double(n.as_f64())
            }
        }
        JsonValue::String(s) => {
            if let Some(bytes) = crate::b64_string_to_bytes(s) {
                Value::Bytes(Some(bytes))
            } else {
                Value::String(Some(s.clone()))
            }
        }
        other => Value::String(Some(other.to_string())),
    }
}

/// Maps a SeaORM cell onto JSON for plan interpretation.
fn sea_to_json(v: &Value) -> JsonValue {
    match v {
        Value::Bool(Some(b)) => JsonValue::Bool(*b),
        Value::TinyInt(Some(n)) => JsonValue::from(*n),
        Value::SmallInt(Some(n)) => JsonValue::from(*n),
        Value::Int(Some(n)) => JsonValue::from(*n),
        Value::BigInt(Some(n)) => JsonValue::from(*n),
        Value::TinyUnsigned(Some(n)) => JsonValue::from(*n),
        Value::SmallUnsigned(Some(n)) => JsonValue::from(*n),
        Value::Unsigned(Some(n)) => JsonValue::from(*n),
        Value::BigUnsigned(Some(n)) => JsonValue::from(*n),
        Value::Float(Some(n)) => JsonValue::from(f64::from(*n)),
        Value::Double(Some(n)) => JsonValue::from(*n),
        Value::String(Some(s)) => JsonValue::String(s.clone()),
        Value::Char(Some(c)) => JsonValue::String(c.to_string()),
        Value::Bytes(Some(b)) => JsonValue::String(crate::bytes_to_b64_string(b)),
        _ => JsonValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::json_to_sea;
    use sea_orm::Value;
    use serde_json::json;

    #[test]
    fn typed_null_bytes_is_bytea_null() {
        assert!(matches!(
            json_to_sea(&json!({ "$sea_null": "Bytes" })),
            Value::Bytes(None)
        ));
    }

    #[test]
    fn typed_null_bigint_is_integer_null() {
        assert!(matches!(
            json_to_sea(&json!({ "$sea_null": "BigInt" })),
            Value::BigInt(None)
        ));
    }

    #[test]
    fn b64_string_decodes_as_bytes() {
        assert!(matches!(
            json_to_sea(&json!("b64:AA==")),
            Value::Bytes(Some(b)) if b.as_slice() == [0]
        ));
    }
}
