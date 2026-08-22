//! Native [`ExecuteRequest`] execution (no JSON `DbAtomicRequest` conversion).
//!
//! [`DbValue::Text`] stays a string even when the payload starts with `b64:`.
//! [`DbValue::Bytes`] maps to SeaORM bytes. Typed nulls use the matching
//! SeaORM `Value::…(None)` variant so column types survive the round trip.

use bookclerk_plugin_abi::v2::encoded_execute_reply_bytes;
use bookclerk_plugin_abi::{
    DbColumn, DbPlanStatementKind, DbResultSelection, DbRow, DbTiming, DbType, DbValue,
    ExecuteReply, ExecuteRequest, StatementResult,
};
use sea_orm::{
    from_query_result_to_proxy_row, ConnectionTrait, DatabaseConnection, DbErr, ProxyRow,
    Statement, TransactionTrait, Value as SeaValue,
};

use crate::exec::{
    collect_capped_query_results, exceeds_result_row_cap, remaining_deadline_ms,
    rows_affected_for_kind, AtomicSession, ExecCaps,
};
use crate::lower_canonical_sql;
use crate::proxy_txn::{
    consume_commit_injection, is_txn_broken, take_txn_fault, with_exec_budget,
    AtomicInterruptPhase, ExecBudget,
};
use crate::{cap_query_sql, record_query_rows_seen};
use std::sync::Arc;
use std::time::Instant;

/// Convert a typed bind into a SeaORM value without JSON / `b64:` decoding.
#[must_use]
pub fn db_value_to_sea(value: &DbValue) -> SeaValue {
    match value {
        DbValue::Null(DbType::Unspecified | DbType::Text) => SeaValue::String(None),
        DbValue::Null(DbType::Int64) => SeaValue::BigInt(None),
        DbValue::Null(DbType::Float64) => SeaValue::Double(None),
        DbValue::Null(DbType::Bytes) => SeaValue::Bytes(None),
        DbValue::Null(DbType::Bool) => SeaValue::Bool(None),
        DbValue::Text(s) => SeaValue::String(Some(s.clone())),
        DbValue::Int64(n) => SeaValue::BigInt(Some(*n)),
        DbValue::Float64(n) => SeaValue::Double(Some(*n)),
        DbValue::Bytes(b) => SeaValue::Bytes(Some(b.clone())),
        DbValue::Boolean(b) => SeaValue::Bool(Some(*b)),
    }
}

/// Reconstruct a typed cell from a SeaORM value.
///
/// Unlike the JSON guest path, this preserves engine types: a BLOB column
/// becomes [`DbValue::Bytes`], and `b64:` text is left as [`DbValue::Text`].
///
/// # Errors
///
/// Returns when the SeaORM value is outside the universal domain.
pub fn db_value_from_sea(v: &SeaValue) -> Result<DbValue, String> {
    match v {
        SeaValue::Bool(Some(b)) => Ok(DbValue::Boolean(*b)),
        SeaValue::TinyInt(Some(n)) => Ok(DbValue::Int64(i64::from(*n))),
        SeaValue::SmallInt(Some(n)) => Ok(DbValue::Int64(i64::from(*n))),
        SeaValue::Int(Some(n)) => Ok(DbValue::Int64(i64::from(*n))),
        SeaValue::BigInt(Some(n)) => Ok(DbValue::Int64(*n)),
        SeaValue::TinyUnsigned(Some(n)) => Ok(DbValue::Int64(i64::from(*n))),
        SeaValue::SmallUnsigned(Some(n)) => Ok(DbValue::Int64(i64::from(*n))),
        SeaValue::Unsigned(Some(n)) => Ok(DbValue::Int64(i64::from(*n))),
        SeaValue::BigUnsigned(Some(n)) => i64::try_from(*n)
            .map(DbValue::Int64)
            .map_err(|_| format!("unsigned integer {n} overflows int64")),
        SeaValue::Float(Some(n)) => {
            let f = f64::from(*n);
            if !f.is_finite() {
                return Err("float64 value is not finite".into());
            }
            Ok(DbValue::Float64(f))
        }
        SeaValue::Double(Some(n)) => {
            if !n.is_finite() {
                return Err("float64 value is not finite".into());
            }
            Ok(DbValue::Float64(*n))
        }
        SeaValue::String(Some(s)) => Ok(DbValue::Text(s.to_string())),
        SeaValue::Char(Some(c)) => Ok(DbValue::Text(c.to_string())),
        SeaValue::Bytes(Some(b)) => Ok(DbValue::Bytes(b.to_vec())),
        SeaValue::ChronoDateTimeUtc(Some(dt)) => Ok(DbValue::Text(dt.to_rfc3339())),
        SeaValue::ChronoDateTime(Some(dt)) => Ok(DbValue::Text(dt.and_utc().to_rfc3339())),
        SeaValue::ChronoDate(Some(d)) => Ok(DbValue::Text(d.to_string())),
        SeaValue::ChronoTime(Some(t)) => Ok(DbValue::Text(t.to_string())),
        SeaValue::ChronoDateTimeWithTimeZone(Some(dt)) => Ok(DbValue::Text(dt.to_rfc3339())),
        SeaValue::ChronoDateTimeLocal(Some(dt)) => Ok(DbValue::Text(dt.to_rfc3339())),
        SeaValue::Uuid(Some(u)) => Ok(DbValue::Text(u.to_string())),
        SeaValue::Json(Some(_)) => Err("json is not a baseline DbValue".into()),
        SeaValue::Enum(_) => Err("enums are not a baseline DbValue".into()),
        SeaValue::Array(_, _) => Err("arrays are not a baseline DbValue".into()),
        SeaValue::Bool(None) => Ok(DbValue::Null(DbType::Bool)),
        SeaValue::TinyInt(None)
        | SeaValue::SmallInt(None)
        | SeaValue::Int(None)
        | SeaValue::BigInt(None)
        | SeaValue::TinyUnsigned(None)
        | SeaValue::SmallUnsigned(None)
        | SeaValue::Unsigned(None)
        | SeaValue::BigUnsigned(None) => Ok(DbValue::Null(DbType::Int64)),
        SeaValue::Float(None) | SeaValue::Double(None) => Ok(DbValue::Null(DbType::Float64)),
        SeaValue::Bytes(None) => Ok(DbValue::Null(DbType::Bytes)),
        SeaValue::String(None)
        | SeaValue::Char(None)
        | SeaValue::ChronoDateTimeUtc(None)
        | SeaValue::ChronoDateTime(None)
        | SeaValue::ChronoDate(None)
        | SeaValue::ChronoTime(None)
        | SeaValue::ChronoDateTimeWithTimeZone(None)
        | SeaValue::ChronoDateTimeLocal(None)
        | SeaValue::Json(None)
        | SeaValue::Uuid(None) => Ok(DbValue::Null(DbType::Text)),
    }
}

/// `DbType` of a non-null cell (typed nulls keep their declared type).
fn db_type_of(v: &DbValue) -> DbType {
    match v {
        DbValue::Null(ty) => *ty,
        DbValue::Boolean(_) => DbType::Bool,
        DbValue::Int64(_) => DbType::Int64,
        DbValue::Float64(_) => DbType::Float64,
        DbValue::Text(_) => DbType::Text,
        DbValue::Bytes(_) => DbType::Bytes,
    }
}

/// UTF-8 / blob byte length of a cell (0 for scalars).
fn db_value_cell_len(v: &DbValue) -> usize {
    match v {
        DbValue::Text(s) => s.len(),
        DbValue::Bytes(b) => b.len(),
        _ => 0,
    }
}

/// Builds a positional [`StatementResult`] from SeaORM proxy rows.
///
/// # Errors
///
/// Returns when a row exceeds the result cap, a cell exceeds `max_cell_bytes`,
/// or a SeaORM value is outside the universal domain.
fn statement_result_from_proxy_rows(
    proxy_rows: Vec<ProxyRow>,
    kind: DbPlanStatementKind,
    caps: ExecCaps,
) -> Result<StatementResult, DbErr> {
    if exceeds_result_row_cap(proxy_rows.len(), caps.max_result_rows) {
        return Err(DbErr::Custom(format!(
            "query returned {} rows; maxResultRows is {}",
            proxy_rows.len(),
            caps.max_result_rows
        )));
    }
    let columns_order: Vec<String> = proxy_rows
        .first()
        .map(|row| row.values.keys().cloned().collect())
        .unwrap_or_default();
    let mut db_columns: Vec<DbColumn> = columns_order
        .iter()
        .map(|name| DbColumn {
            name: name.clone(),
            db_type: DbType::Unspecified,
        })
        .collect();
    let mut db_rows = Vec::with_capacity(proxy_rows.len());
    for proxy in &proxy_rows {
        let mut values = Vec::with_capacity(columns_order.len());
        for name in &columns_order {
            let sea = proxy
                .values
                .get(name)
                .ok_or_else(|| DbErr::Custom(format!("result row missing column `{name}`")))?;
            let cell = db_value_from_sea(sea).map_err(DbErr::Custom)?;
            if caps.max_cell_bytes > 0 {
                let n = db_value_cell_len(&cell);
                let cap = usize::try_from(caps.max_cell_bytes).unwrap_or(usize::MAX);
                if n > cap {
                    return Err(DbErr::Custom(format!(
                        "column `{name}` is {n} bytes; maxCellBytes is {}",
                        caps.max_cell_bytes
                    )));
                }
            }
            values.push(cell);
        }
        db_rows.push(DbRow { values });
    }
    for (i, col) in db_columns.iter_mut().enumerate() {
        if let Some(first) = db_rows.first() {
            if let Some(cell) = first.values.get(i) {
                col.db_type = db_type_of(cell);
            }
        }
    }
    let mut result = StatementResult::from_rows(db_columns, db_rows).map_err(DbErr::Custom)?;
    result.rows_affected = rows_affected_for_kind(kind, result.rows.len());
    Ok(result)
}

/// Run a typed atomic batch on an existing SeaORM connection.
///
/// Encodes the [`ExecuteReply`] **before** COMMIT. Encoding or result-budget
/// failures roll the transaction back.
///
/// # Errors
///
/// Returns [`DbErr`] when a statement fails, the encoded reply exceeds
/// `max_atomic_result_bytes`, or the session is interrupted.
pub async fn execute_typed_on_session(
    db: &DatabaseConnection,
    req: &ExecuteRequest,
    timing_source: &str,
    caps: impl Into<ExecCaps>,
    session: AtomicSession,
) -> Result<ExecuteReply, DbErr> {
    let caps = caps.into();
    session.check(AtomicInterruptPhase::BeforeBegin)?;
    let budget = ExecBudget::new(session.deadline_unix_ms, caps.max_result_rows);
    let seen_budget = Arc::clone(&budget);
    let result = with_exec_budget(Arc::clone(&budget), || {
        execute_typed_body(db, req, timing_source, caps, session)
    })
    .await;
    record_query_rows_seen(seen_budget.rows_seen());
    result
}

/// Transaction body for [`execute_typed_on_session`]: run, encode, then COMMIT.
///
/// # Errors
///
/// Returns [`DbErr`] when a statement fails, encoding fails, or COMMIT fails.
async fn execute_typed_body(
    db: &DatabaseConnection,
    req: &ExecuteRequest,
    timing_source: &str,
    caps: ExecCaps,
    session: AtomicSession,
) -> Result<ExecuteReply, DbErr> {
    if req.statements.is_empty() {
        return Err(DbErr::Custom(
            "executeAtomic statements must be non-empty".into(),
        ));
    }
    let started = Instant::now();
    let txn = db.begin().await?;
    if is_txn_broken() {
        let _ = txn.rollback().await;
        let fault = take_txn_fault().unwrap_or_else(|| "database begin failed".into());
        return Err(DbErr::Custom(fault));
    }
    let backend = ConnectionTrait::get_database_backend(&txn);
    if backend == sea_orm::DatabaseBackend::Postgres {
        if let Some(ms) = remaining_deadline_ms(session.deadline_unix_ms) {
            let sql = format!("SET LOCAL statement_timeout = '{ms}ms'");
            if let Err(err) = txn.execute_raw(Statement::from_string(backend, sql)).await {
                let _ = txn.rollback().await;
                return Err(err);
            }
        }
    }
    let sql_started = Instant::now();
    let mut statements = Vec::with_capacity(req.statements.len());
    for stmt in &req.statements {
        if let Err(err) = session.check(AtomicInterruptPhase::BetweenStatements) {
            let _ = txn.rollback().await;
            let _ = take_txn_fault();
            return Err(err);
        }
        let values: Vec<SeaValue> = stmt.parameters.iter().map(db_value_to_sea).collect();
        let sql = if stmt.kind.wrap_select_limit() {
            cap_query_sql(&stmt.sql, caps.max_result_rows)
        } else {
            stmt.sql.clone()
        };
        let sql = lower_canonical_sql(backend, &sql);
        let sea_stmt = Statement::from_sql_and_values(backend, &sql, values);
        let stmt_result =
            match stmt.result_selection {
                DbResultSelection::Rows | DbResultSelection::Cursor => {
                    let engine_rows =
                        match collect_capped_query_results(&txn, sea_stmt, caps.max_result_rows)
                            .await
                        {
                            Ok(rows) => rows,
                            Err(err) => {
                                let _ = txn.rollback().await;
                                let _ = take_txn_fault();
                                return Err(err);
                            }
                        };
                    let proxy_rows: Vec<ProxyRow> = engine_rows
                        .iter()
                        .map(from_query_result_to_proxy_row)
                        .collect();
                    match statement_result_from_proxy_rows(proxy_rows, stmt.kind, caps) {
                        Ok(result) => result,
                        Err(err) => {
                            let _ = txn.rollback().await;
                            let _ = take_txn_fault();
                            return Err(err);
                        }
                    }
                }
                DbResultSelection::AffectedRows | DbResultSelection::Discard => {
                    let exec = match txn.execute_raw(sea_stmt).await {
                        Ok(exec) => exec,
                        Err(err) => {
                            let _ = txn.rollback().await;
                            let _ = take_txn_fault();
                            return Err(err);
                        }
                    };
                    if matches!(stmt.result_selection, DbResultSelection::Discard) {
                        StatementResult::from_affected(0)
                    } else {
                        StatementResult::from_affected(exec.rows_affected())
                    }
                }
            };
        statements.push(stmt_result);
    }
    if consume_commit_injection() {
        let _ = txn.rollback().await;
        let _ = take_txn_fault();
        return Err(DbErr::Custom(
            "database commit failed: injected commit failure".into(),
        ));
    }
    if let Err(err) = session.check(AtomicInterruptPhase::AroundCommit) {
        let _ = txn.rollback().await;
        let _ = take_txn_fault();
        return Err(err);
    }
    let db_execution_us = u64::try_from(sql_started.elapsed().as_micros()).unwrap_or(u64::MAX);
    let reply = ExecuteReply {
        operation_id: req.operation_id.clone(),
        statements,
        timing: DbTiming {
            attempt_elapsed_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            db_execution_us,
            db_timing_source: timing_source.to_string(),
        },
    };
    if let Err(err) = reply.validate_positional() {
        let _ = txn.rollback().await;
        let _ = take_txn_fault();
        return Err(DbErr::Custom(err));
    }
    match encoded_execute_reply_bytes(&reply) {
        Ok(bytes) => {
            if caps.max_atomic_result_bytes > 0 {
                let used = bytes.len();
                let cap = usize::try_from(caps.max_atomic_result_bytes).unwrap_or(usize::MAX);
                if used > cap {
                    let _ = txn.rollback().await;
                    let _ = take_txn_fault();
                    return Err(DbErr::Custom(format!(
                        "atomic result is {used} bytes; maxAtomicResultBytes is {}",
                        caps.max_atomic_result_bytes
                    )));
                }
            }
        }
        Err(err) => {
            let _ = txn.rollback().await;
            let _ = take_txn_fault();
            return Err(DbErr::Custom(format!(
                "failed to encode ExecuteReply before COMMIT: {err}"
            )));
        }
    }
    txn.commit().await.map_err(|err| {
        let _ = take_txn_fault();
        DbErr::Custom(format!("database commit failed: {err}"))
    })?;
    if let Some(fault) = take_txn_fault() {
        return Err(DbErr::Custom(fault));
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_starting_with_b64_stays_text() {
        let v = db_value_to_sea(&DbValue::Text("b64:AAAA".into()));
        match v {
            SeaValue::String(Some(s)) => assert_eq!(&*s, "b64:AAAA"),
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn bytes_stay_bytes() {
        let v = db_value_to_sea(&DbValue::Bytes(vec![0, 1, 2]));
        match v {
            SeaValue::Bytes(Some(b)) => assert_eq!(&*b, &[0, 1, 2]),
            other => panic!("expected Bytes, got {other:?}"),
        }
    }

    #[test]
    fn typed_nulls_use_matching_sea_variants() {
        assert!(matches!(
            db_value_to_sea(&DbValue::Null(DbType::Bytes)),
            SeaValue::Bytes(None)
        ));
        assert!(matches!(
            db_value_to_sea(&DbValue::Null(DbType::Int64)),
            SeaValue::BigInt(None)
        ));
        assert!(matches!(
            db_value_to_sea(&DbValue::Null(DbType::Bool)),
            SeaValue::Bool(None)
        ));
        assert!(matches!(
            db_value_to_sea(&DbValue::Null(DbType::Float64)),
            SeaValue::Double(None)
        ));
        assert!(matches!(
            db_value_to_sea(&DbValue::Null(DbType::Text)),
            SeaValue::String(None)
        ));
    }
}
