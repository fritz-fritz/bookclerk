//! Run a generic [`DbAtomicPlan`] on a SeaORM connection (one native transaction).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bookclerk_plugin_abi::{
    DbAtomicPlan, DbAtomicTiming, DbPlanExecResult, DbPlanStatementKind, DbPlanStmtExecResult,
};
use sea_orm::{
    from_query_result_to_proxy_row, ConnectionTrait, DatabaseConnection, Statement,
    TransactionTrait, Value,
};
use serde_json::Value as JsonValue;

use super::interpret::interpret_exec;
use crate::atomic_ops::DbAtomicResult;
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
    execute_plan_on_capped(db, plan, expected_hash, operation_id, timing_source, 0).await
}

/// Like [`execute_plan_on`], failing when a statement returns more than `max_result_rows`.
///
/// `max_result_rows` of `0` means unlimited.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when a statement fails or exceeds the row cap.
pub async fn execute_plan_on_capped(
    db: &DatabaseConnection,
    plan: &DbAtomicPlan,
    expected_hash: &str,
    operation_id: &str,
    timing_source: &str,
    max_result_rows: u32,
) -> Result<DbAtomicResult> {
    let exec =
        execute_statements_on(db, plan, operation_id, timing_source, max_result_rows).await?;
    Ok(interpret_exec(plan, &exec, expected_hash))
}

/// Session-level cancel / deadline for one atomic attempt (not hashed).
#[derive(Clone, Default)]
pub struct AtomicSession {
    /// Host RPC cancel flag, when the guest shares a process with the host.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Guest-visible deadline (`deadlineUnixMs` on the wire).
    pub deadline_unix_ms: Option<u64>,
}

impl AtomicSession {
    /// Builds session control from a wire `deadlineUnixMs`.
    #[must_use]
    pub fn from_deadline(deadline_unix_ms: Option<u64>) -> Self {
        Self {
            cancel: None,
            deadline_unix_ms,
        }
    }
}

/// Executes `plan` as one transaction and returns generic statement results.
///
/// Guests and in-process tests share this path. `max_result_rows` of `0` means
/// unlimited; otherwise a statement that yields more rows fails the plan
/// (the transaction is rolled back) rather than truncating the result.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when a statement fails.
pub async fn execute_statements_on(
    db: &DatabaseConnection,
    plan: &DbAtomicPlan,
    operation_id: &str,
    timing_source: &str,
    max_result_rows: u32,
) -> Result<DbPlanExecResult> {
    execute_statements_on_session(
        db,
        plan,
        operation_id,
        timing_source,
        max_result_rows,
        AtomicSession::default(),
    )
    .await
}

/// [`execute_statements_on`] with session cancel / deadline checks.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when a statement fails or the session is interrupted.
pub async fn execute_statements_on_session(
    db: &DatabaseConnection,
    plan: &DbAtomicPlan,
    operation_id: &str,
    timing_source: &str,
    max_result_rows: u32,
    session: AtomicSession,
) -> Result<DbPlanExecResult> {
    session.check(crate::AtomicInterruptPhase::BeforeBegin)?;
    let started = Instant::now();
    let txn = db.begin().await.map_err(LibraryError::Orm)?;
    if crate::is_txn_broken() {
        let _ = txn.rollback().await;
        let fault = crate::take_txn_fault().unwrap_or_else(|| "database begin failed".into());
        return Err(LibraryError::Orm(sea_orm::DbErr::Custom(fault)));
    }
    let sql_started = Instant::now();
    let mut statements = Vec::with_capacity(plan.statements.len());
    let backend = txn.get_database_backend();
    for stmt in &plan.statements {
        if let Err(err) = session.check(crate::AtomicInterruptPhase::BetweenStatements) {
            let _ = txn.rollback().await;
            let _ = crate::take_txn_fault();
            return Err(err);
        }
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
                let json_rows: Vec<JsonValue> = rows
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
                if exceeds_result_row_cap(json_rows.len(), max_result_rows) {
                    let _ = txn.rollback().await;
                    let _ = crate::take_txn_fault();
                    return Err(LibraryError::Orm(sea_orm::DbErr::Custom(format!(
                        "query returned {} rows; maxResultRows is {max_result_rows}",
                        json_rows.len()
                    ))));
                }
                let rows_affected = u64::try_from(json_rows.len()).unwrap_or(u64::MAX);
                DbPlanStmtExecResult {
                    rows: json_rows,
                    rows_affected,
                }
            }
            DbPlanStatementKind::Execute => {
                let exec = match txn.execute_raw(sea_stmt).await {
                    Ok(exec) => exec,
                    Err(err) => {
                        let _ = txn.rollback().await;
                        let _ = crate::take_txn_fault();
                        return Err(LibraryError::Orm(err));
                    }
                };
                DbPlanStmtExecResult {
                    rows: Vec::new(),
                    rows_affected: exec.rows_affected(),
                }
            }
        };
        statements.push(stmt_result);
    }
    if crate::consume_commit_injection() {
        let _ = txn.rollback().await;
        let _ = crate::take_txn_fault();
        return Err(LibraryError::Orm(sea_orm::DbErr::Custom(
            "database commit failed: injected commit failure".into(),
        )));
    }
    if let Err(err) = session.check(crate::AtomicInterruptPhase::AroundCommit) {
        let _ = txn.rollback().await;
        let _ = crate::take_txn_fault();
        return Err(err);
    }
    txn.commit().await.map_err(|err| {
        let _ = crate::take_txn_fault();
        LibraryError::Orm(sea_orm::DbErr::Custom(format!(
            "database commit failed: {err}"
        )))
    })?;
    if let Some(fault) = crate::take_txn_fault() {
        return Err(LibraryError::Orm(sea_orm::DbErr::Custom(fault)));
    }
    let db_execution_us = u64::try_from(sql_started.elapsed().as_micros()).unwrap_or(u64::MAX);
    Ok(DbPlanExecResult {
        operation_id: operation_id.to_string(),
        statements,
        timing: Some(DbAtomicTiming {
            attempt_elapsed_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            db_execution_us: Some(db_execution_us),
            db_timing_source: Some(timing_source.to_string()),
        }),
    })
}

impl AtomicSession {
    /// Checks cancel / deadline / test inject at `phase`.
    fn check(&self, phase: crate::AtomicInterruptPhase) -> Result<()> {
        use crate::AtomicInterruptKind;
        if let Some(kind) = crate::consume_atomic_interrupt(phase) {
            return Err(interrupt_err(phase, kind));
        }
        let cancelled = self
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst));
        let expired = self.deadline_unix_ms.is_some_and(|ms| unix_now_ms() >= ms);
        if cancelled {
            return Err(interrupt_err(phase, AtomicInterruptKind::Cancel));
        }
        if expired {
            return Err(interrupt_err(phase, AtomicInterruptKind::Deadline));
        }
        Ok(())
    }
}

/// Maps a session interrupt onto a guest-classifiable [`LibraryError`].
fn interrupt_err(
    phase: crate::AtomicInterruptPhase,
    kind: crate::AtomicInterruptKind,
) -> LibraryError {
    use crate::{AtomicInterruptKind, AtomicInterruptPhase};
    let around_commit = matches!(phase, AtomicInterruptPhase::AroundCommit);
    let msg = match (around_commit, kind) {
        (true, _) => "database commit failed: session interrupt at commit",
        (false, AtomicInterruptKind::Cancel) => "cancelled: atomic session cancelled",
        (false, AtomicInterruptKind::Deadline) => "deadline_exceeded: atomic deadline elapsed",
    };
    LibraryError::Orm(sea_orm::DbErr::Custom(msg.into()))
}

/// Current unix time in milliseconds.
fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// True when `n` exceeds a positive `max_result_rows` cap.
fn exceeds_result_row_cap(n: usize, max_result_rows: u32) -> bool {
    if max_result_rows == 0 {
        return false;
    }
    let cap = usize::try_from(max_result_rows).unwrap_or(usize::MAX);
    n > cap
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
