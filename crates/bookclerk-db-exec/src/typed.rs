//! Native [`ExecuteRequest`] execution (no JSON `DbAtomicRequest` conversion).
//!
//! [`DbValue::Text`] stays a string even when the payload starts with `b64:`.
//! [`DbValue::Bytes`] maps to SeaORM bytes. Typed nulls use the matching
//! SeaORM `Value::…(None)` variant so column types survive the round trip.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use bookclerk_plugin_abi::v2::{encoded_execute_reply_bytes, encoded_statement_result_bytes};
use bookclerk_plugin_abi::{
    DbColumn, DbPlanStatementKind, DbResultSelection, DbRow, DbTiming, DbType, DbValue,
    ExecuteReply, ExecuteRequest, StatementResult,
};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, QueryResult, Statement, TransactionTrait,
    Value as SeaValue,
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
use crate::{
    cap_query_sql, record_query_rows_seen, set_positional_result_columns,
    take_positional_result_columns,
};

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

/// Builds a positional [`StatementResult`] from engine [`QueryResult`]s.
///
/// Column names and declared types come from rusqlite/SQLite metadata when the
/// adapter recorded them ([`take_positional_result_columns`]), otherwise from
/// the first engine row (Postgres `type_info`, else `column_names`). Duplicate
/// names are rejected here, before any name-keyed map conversion. Empty
/// Postgres `SELECT`s record metadata via a one-row probe first.
///
/// # Errors
///
/// Returns when a row exceeds the result cap, a cell exceeds `max_cell_bytes`,
/// the encoded statement exceeds `max_result_bytes`, a SeaORM value is outside
/// the universal domain, or column names are duplicated.
fn statement_result_from_query_results(
    engine_rows: &[QueryResult],
    kind: DbPlanStatementKind,
    caps: ExecCaps,
) -> Result<StatementResult, DbErr> {
    if exceeds_result_row_cap(engine_rows.len(), caps.max_result_rows) {
        return Err(DbErr::Custom(format!(
            "query returned {} rows; maxResultRows is {}",
            engine_rows.len(),
            caps.max_result_rows
        )));
    }
    let mut db_columns = take_positional_result_columns().unwrap_or_else(|| {
        engine_rows
            .first()
            .map(db_columns_from_engine_row)
            .unwrap_or_default()
    });
    reject_duplicate_column_names(&db_columns)?;
    let mut db_rows = Vec::with_capacity(engine_rows.len());
    for engine in engine_rows {
        let values = db_values_from_query_result(engine, &db_columns)?;
        if caps.max_cell_bytes > 0 {
            let cap = usize::try_from(caps.max_cell_bytes).unwrap_or(usize::MAX);
            for (col, cell) in db_columns.iter().zip(values.iter()) {
                let n = db_value_cell_len(cell);
                if n > cap {
                    return Err(DbErr::Custom(format!(
                        "column `{}` is {n} bytes; maxCellBytes is {}",
                        col.name, caps.max_cell_bytes
                    )));
                }
            }
        }
        db_rows.push(DbRow { values });
    }
    for (i, col) in db_columns.iter_mut().enumerate() {
        if col.db_type != DbType::Unspecified {
            continue;
        }
        for row in &db_rows {
            if let Some(cell) = row.values.get(i) {
                if !matches!(cell, DbValue::Null(_)) {
                    col.db_type = db_type_of(cell);
                    break;
                }
            }
        }
        if col.db_type == DbType::Unspecified {
            if let Some(DbValue::Null(ty)) = db_rows.first().and_then(|r| r.values.get(i)) {
                col.db_type = *ty;
            }
        }
    }
    let mut result = StatementResult::from_rows(db_columns, db_rows).map_err(DbErr::Custom)?;
    result.rows_affected = rows_affected_for_kind(kind, result.rows.len());
    reject_statement_result_bytes(&result, caps.max_result_bytes)?;
    Ok(result)
}

/// Records column names/types for a Postgres `SELECT` that returned no rows.
///
/// sqlx only materializes `RowDescription` on a `PgRow`. Extended-query
/// `prepare` (Parse + Describe, no Execute) returns the same field metadata
/// without running the statement, so volatile functions and data-modifying
/// CTEs are not evaluated a second time.
///
/// # Errors
///
/// Returns when the driver cannot describe the statement.
async fn record_postgres_empty_result_columns(
    db: &DatabaseConnection,
    sql: &str,
) -> Result<(), DbErr> {
    use sea_orm::sqlx::{AssertSqlSafe, Column, Executor, SqlSafeStr, Statement, TypeInfo};
    let pool = db.get_postgres_connection_pool();
    let prepared = pool
        .prepare(AssertSqlSafe(sql.to_owned()).into_sql_str())
        .await
        .map_err(|err| DbErr::Custom(format!("postgres prepare/describe: {err}")))?;
    let columns = prepared
        .columns()
        .iter()
        .map(|c| DbColumn {
            name: c.name().to_string(),
            db_type: db_type_from_pg_type_name(c.type_info().name()),
        })
        .collect();
    set_positional_result_columns(columns);
    Ok(())
}

/// Positional [`DbColumn`]s from one engine row (Postgres OIDs when present).
fn db_columns_from_engine_row(row: &QueryResult) -> Vec<DbColumn> {
    if let Some(pg) = row.try_as_pg_row() {
        use sea_orm::sqlx::{Column, Row, TypeInfo};
        return pg
            .columns()
            .iter()
            .map(|c| DbColumn {
                name: c.name().to_string(),
                db_type: db_type_from_pg_type_name(c.type_info().name()),
            })
            .collect();
    }
    row.column_names()
        .into_iter()
        .map(|name| DbColumn {
            name,
            db_type: DbType::Unspecified,
        })
        .collect()
}

/// Maps a sqlx Postgres `TypeInfo::name` onto the universal [`DbType`].
fn db_type_from_pg_type_name(name: &str) -> DbType {
    match name {
        "BOOL" => DbType::Bool,
        "INT2" | "INT4" | "INT8" | "SMALLINT" | "INT" | "INTEGER" | "BIGINT" | "SMALLSERIAL"
        | "SERIAL" | "BIGSERIAL" | "OID" => DbType::Int64,
        "FLOAT4" | "FLOAT8" | "REAL" | "DOUBLE PRECISION" => DbType::Float64,
        "BYTEA" => DbType::Bytes,
        "TEXT" | "VARCHAR" | "NAME" | "BPCHAR" | "CHAR" | "CSTRING" | "UNKNOWN" => DbType::Text,
        _ => DbType::Unspecified,
    }
}

/// Fails when two positional columns share a name.
fn reject_duplicate_column_names(columns: &[DbColumn]) -> Result<(), DbErr> {
    let mut seen = HashSet::new();
    for col in columns {
        if !col.name.is_empty() && !seen.insert(col.name.as_str()) {
            return Err(DbErr::Custom(format!(
                "duplicate column name `{}`",
                col.name
            )));
        }
    }
    Ok(())
}

/// Fails when the Cap'n-encoded [`StatementResult`] exceeds `max_result_bytes`.
fn reject_statement_result_bytes(
    result: &StatementResult,
    max_result_bytes: u32,
) -> Result<(), DbErr> {
    if max_result_bytes == 0 {
        return Ok(());
    }
    let used = encoded_statement_result_bytes(result)
        .map(|b| b.len())
        .unwrap_or(usize::MAX);
    let cap = usize::try_from(max_result_bytes).unwrap_or(usize::MAX);
    if used > cap {
        return Err(DbErr::Custom(format!(
            "query result is {used} bytes; maxResultBytes is {max_result_bytes}"
        )));
    }
    Ok(())
}

/// Positional cells from one engine row (proxy rows looked up by recorded name).
fn db_values_from_query_result(
    row: &QueryResult,
    columns: &[DbColumn],
) -> Result<Vec<DbValue>, DbErr> {
    if let Some(proxy) = row.try_as_proxy_row() {
        let mut values = Vec::with_capacity(columns.len());
        for col in columns {
            let sea = proxy.values.get(&col.name).ok_or_else(|| {
                DbErr::Custom(format!("result row missing column `{}`", col.name))
            })?;
            values.push(db_value_for_column(sea, col)?);
        }
        return Ok(values);
    }
    let mut values = Vec::with_capacity(columns.len());
    for (i, col) in columns.iter().enumerate() {
        let sea = sea_value_from_index(row, i, col.db_type)?;
        values.push(db_value_for_column(&sea, col)?);
    }
    Ok(values)
}

/// Converts a SeaORM cell, stamping declared column type onto SQL NULL.
fn db_value_for_column(sea: &SeaValue, col: &DbColumn) -> Result<DbValue, DbErr> {
    let value = db_value_from_sea(sea).map_err(DbErr::Custom)?;
    Ok(match value {
        DbValue::Null(_) if col.db_type != DbType::Unspecified => DbValue::Null(col.db_type),
        other => other,
    })
}

/// Decodes one positional cell without going through a name-keyed map.
///
/// `Option<T>` succeeds for every SQL NULL, so the declared [`DbType`] is tried
/// first. Untyped nulls stay `Null(Unspecified)` rather than the first match
/// (`Bytes`).
fn sea_value_from_index(row: &QueryResult, idx: usize, prefer: DbType) -> Result<SeaValue, DbErr> {
    let order: &[DbType] = match prefer {
        DbType::Bytes => &[
            DbType::Bytes,
            DbType::Int64,
            DbType::Float64,
            DbType::Text,
            DbType::Bool,
        ],
        DbType::Int64 | DbType::Bool => &[
            DbType::Int64,
            DbType::Bool,
            DbType::Float64,
            DbType::Text,
            DbType::Bytes,
        ],
        DbType::Float64 => &[
            DbType::Float64,
            DbType::Int64,
            DbType::Text,
            DbType::Bytes,
            DbType::Bool,
        ],
        DbType::Text | DbType::Unspecified => &[
            DbType::Int64,
            DbType::Float64,
            DbType::Text,
            DbType::Bool,
            DbType::Bytes,
        ],
    };
    let mut saw_null = false;
    for ty in order {
        match ty {
            DbType::Bytes => {
                if let Ok(v) = row.try_get_by_index::<Option<Vec<u8>>>(idx) {
                    if v.is_some() {
                        return Ok(SeaValue::Bytes(v));
                    }
                    saw_null = true;
                }
            }
            DbType::Int64 => {
                if let Ok(v) = row.try_get_by_index::<Option<i64>>(idx) {
                    if v.is_some() {
                        return Ok(SeaValue::BigInt(v));
                    }
                    saw_null = true;
                }
                if let Ok(v) = row.try_get_by_index::<Option<i32>>(idx) {
                    if let Some(n) = v {
                        return Ok(SeaValue::BigInt(Some(i64::from(n))));
                    }
                    saw_null = true;
                }
                if let Ok(v) = row.try_get_by_index::<Option<i16>>(idx) {
                    if let Some(n) = v {
                        return Ok(SeaValue::BigInt(Some(i64::from(n))));
                    }
                    saw_null = true;
                }
            }
            DbType::Float64 => {
                if let Ok(v) = row.try_get_by_index::<Option<f64>>(idx) {
                    if v.is_some() {
                        return Ok(SeaValue::Double(v));
                    }
                    saw_null = true;
                }
            }
            DbType::Text => {
                if let Ok(v) = row.try_get_by_index::<Option<String>>(idx) {
                    if v.is_some() {
                        return Ok(SeaValue::String(v));
                    }
                    saw_null = true;
                }
            }
            DbType::Bool => {
                if let Ok(v) = row.try_get_by_index::<Option<bool>>(idx) {
                    if v.is_some() {
                        return Ok(SeaValue::Bool(v));
                    }
                    saw_null = true;
                }
            }
            DbType::Unspecified => {}
        }
    }
    if saw_null {
        return Ok(match prefer {
            DbType::Bytes => SeaValue::Bytes(None),
            DbType::Int64 | DbType::Bool => SeaValue::BigInt(None),
            DbType::Float64 => SeaValue::Double(None),
            DbType::Text | DbType::Unspecified => SeaValue::String(None),
        });
    }
    Err(DbErr::Custom(format!(
        "column {idx} is outside the universal DbValue domain"
    )))
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
                    if engine_rows.is_empty()
                        && backend == sea_orm::DatabaseBackend::Postgres
                        && stmt.kind.wrap_select_limit()
                    {
                        if let Err(err) = record_postgres_empty_result_columns(db, &sql).await {
                            let _ = txn.rollback().await;
                            let _ = take_txn_fault();
                            return Err(err);
                        }
                    }
                    match statement_result_from_query_results(&engine_rows, stmt.kind, caps) {
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
                        let result = StatementResult::from_affected(exec.rows_affected());
                        if let Err(err) =
                            reject_statement_result_bytes(&result, caps.max_result_bytes)
                        {
                            let _ = txn.rollback().await;
                            let _ = take_txn_fault();
                            return Err(err);
                        }
                        result
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
    fn reject_duplicate_column_names_fails_closed() {
        let cols = [
            DbColumn {
                name: "n".into(),
                db_type: DbType::Int64,
            },
            DbColumn {
                name: "n".into(),
                db_type: DbType::Int64,
            },
        ];
        assert!(reject_duplicate_column_names(&cols).is_err());
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

    #[test]
    fn typed_null_inherits_declared_column_type() {
        let col = DbColumn {
            name: "x".into(),
            db_type: DbType::Int64,
        };
        let v = db_value_for_column(&SeaValue::Bytes(None), &col).unwrap();
        assert!(matches!(v, DbValue::Null(DbType::Int64)));
    }

    #[test]
    fn postgres_type_names_map_onto_universal_db_type() {
        assert_eq!(db_type_from_pg_type_name("INT4"), DbType::Int64);
        assert_eq!(db_type_from_pg_type_name("INT8"), DbType::Int64);
        assert_eq!(db_type_from_pg_type_name("TEXT"), DbType::Text);
        assert_eq!(db_type_from_pg_type_name("BYTEA"), DbType::Bytes);
        assert_eq!(db_type_from_pg_type_name("BOOL"), DbType::Bool);
        assert_eq!(db_type_from_pg_type_name("FLOAT8"), DbType::Float64);
        assert_eq!(db_type_from_pg_type_name("INTERVAL"), DbType::Unspecified);
        assert_eq!(db_type_from_pg_type_name("NUMERIC"), DbType::Unspecified);
    }
}
