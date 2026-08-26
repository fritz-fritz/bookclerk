//! Native [`ExecuteRequest`] execution (no JSON `DbAtomicRequest` conversion).
//!
//! [`DbValue::Text`] stays a string even when the payload starts with `b64:`.
//! [`DbValue::Bytes`] maps to SeaORM bytes. Typed nulls use the matching
//! SeaORM `Value::…(None)` variant so column types survive the round trip.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use bookclerk_plugin_abi::GuestReceiptPersist;
use bookclerk_plugin_abi::{encoded_execute_reply_bytes, encoded_statement_result_bytes};
use bookclerk_plugin_abi::{
    DbColumn, DbPlanStatementKind, DbResultSelection, DbRow, DbTiming, DbType, DbValue,
    ExecuteReply, ExecuteRequest, StatementResult, TypedDbStatement,
};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbErr, QueryResult, Statement,
    TransactionTrait, Value as SeaValue,
};

use crate::exec::{
    collect_capped_query_results, exceeds_result_row_cap, remaining_deadline_ms,
    rows_affected_for_kind, AtomicSession, ExecCaps,
};
use crate::lower_canonical_sql;
use crate::proxy_txn::{
    consume_commit_injection, consume_savepoint_release_injection,
    consume_savepoint_rollback_injection, is_txn_broken, note_commit_failed, take_txn_fault,
    with_exec_budget, AtomicInterruptPhase, ExecBudget,
};
use crate::schema_postgres::expand_host_schema_execute_request;
use crate::{
    cap_query_sql, record_query_rows_seen, set_positional_result_columns,
    take_positional_result_columns,
};

/// Proven row bound for one statement: `maxRows` when set, otherwise the
/// negotiated adapter cap. Zero on either side means "unlimited".
fn effective_row_cap(stmt_max: u32, caps_max: u32) -> u32 {
    match (stmt_max, caps_max) {
        (0, c) => c,
        (s, 0) => s,
        (s, c) => s.min(c),
    }
}

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
    row_cap: u32,
) -> Result<StatementResult, DbErr> {
    if exceeds_result_row_cap(engine_rows.len(), row_cap) {
        return Err(DbErr::Custom(format!(
            "query returned {} rows; maxRows/maxResultRows is {row_cap}",
            engine_rows.len(),
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
///
/// # Errors
///
/// Returns [`DbErr::Custom`] when a duplicate column name is present.
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
///
/// # Errors
///
/// Returns [`DbErr::Custom`] when the encoded result exceeds `max_result_bytes`.
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
///
/// # Errors
///
/// Returns [`DbErr`] when a column is missing or a cell cannot be decoded.
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

/// Converts a SeaORM cell, normalizing against the declared column type.
///
/// Declared metadata decides the observable variant (typed NULLs, `Boolean`
/// for `0`/`1` in BOOL columns) so every adapter reports identical
/// `DbValue`s — see
/// [`bookclerk_plugin_abi::normalize_db_value_for_column`].
///
/// # Errors
///
/// Returns [`DbErr`] when the SeaORM value is outside the universal domain.
fn db_value_for_column(sea: &SeaValue, col: &DbColumn) -> Result<DbValue, DbErr> {
    let value = db_value_from_sea(sea).map_err(DbErr::Custom)?;
    Ok(bookclerk_plugin_abi::normalize_db_value_for_column(
        value,
        col.db_type,
    ))
}

/// Decodes one positional cell without going through a name-keyed map.
///
/// `Option<T>` succeeds for every SQL NULL, so the declared [`DbType`] is tried
/// first. Untyped nulls stay `Null(Unspecified)` rather than the first match
/// (`Bytes`).
///
/// # Errors
///
/// Returns [`DbErr`] when the cell cannot be decoded for any preferred type.
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
    guest_receipt: GuestReceiptPersist,
    timing_source: &str,
    caps: impl Into<ExecCaps>,
    session: AtomicSession,
) -> Result<ExecuteReply, DbErr> {
    if guest_receipt.is_absent() {
        let caps = caps.into();
        session.check(AtomicInterruptPhase::BeforeBegin)?;
        let budget = ExecBudget::new(session.deadline_unix_ms, caps.max_result_rows);
        let seen_budget = Arc::clone(&budget);
        let result = with_exec_budget(Arc::clone(&budget), || {
            execute_typed_body(
                db,
                req,
                timing_source,
                caps,
                session,
                None::<fn(ExecuteReply) -> Result<Vec<TypedDbStatement>, DbErr>>,
            )
        })
        .await;
        record_query_rows_seen(seen_budget.rows_seen());
        return result;
    }
    let hint = guest_receipt;
    execute_typed_on_session_then(db, req, timing_source, caps, session, move |partial| {
        crate::guest_receipt::guest_receipt_finalize_stmts(
            &partial,
            usize::try_from(hint.guest_statement_len).unwrap_or(usize::MAX),
            &hint.guest_request_hash,
        )
    })
    .await
}

/// Like [`execute_typed_on_session`], running extra statements in the same transaction
/// before COMMIT (used to persist guest replay payloads on `db_atomic_receipts`).
///
/// # Errors
///
/// Returns [`DbErr`] when a statement fails, `then` fails, encoding fails, or COMMIT fails.
pub async fn execute_typed_on_session_then<F>(
    db: &DatabaseConnection,
    req: &ExecuteRequest,
    timing_source: &str,
    caps: impl Into<ExecCaps>,
    session: AtomicSession,
    then: F,
) -> Result<ExecuteReply, DbErr>
where
    F: FnOnce(ExecuteReply) -> Result<Vec<TypedDbStatement>, DbErr>,
{
    let caps = caps.into();
    session.check(AtomicInterruptPhase::BeforeBegin)?;
    let budget = ExecBudget::new(session.deadline_unix_ms, caps.max_result_rows);
    let seen_budget = Arc::clone(&budget);
    let result = with_exec_budget(Arc::clone(&budget), || {
        execute_typed_body(db, req, timing_source, caps, session, Some(then))
    })
    .await;
    record_query_rows_seen(seen_budget.rows_seen());
    result
}

/// Run a typed batch on an already-open transaction (no BEGIN/COMMIT).
///
/// Used by nested SeaORM work: the guest interactive txn is already open, so
/// a second `executeAtomic` BEGIN would fail. The batch runs inside a
/// `SAVEPOINT`; statement, encoding, or budget failures roll back to that
/// savepoint so a later outer `commit()` cannot persist a partial batch.
///
/// # Errors
///
/// Returns [`DbErr`] when a statement fails or the encoded reply exceeds
/// `max_atomic_result_bytes`.
pub async fn execute_typed_on_txn(
    txn: &DatabaseTransaction,
    req: &ExecuteRequest,
    timing_source: &str,
    caps: impl Into<ExecCaps>,
    session: AtomicSession,
    describe: Option<&DatabaseConnection>,
) -> Result<ExecuteReply, DbErr> {
    let caps = caps.into();
    if req.statements.is_empty() {
        return Err(DbErr::Custom(
            "executeAtomic statements must be non-empty".into(),
        ));
    }
    session.check(AtomicInterruptPhase::BetweenStatements)?;
    let budget = ExecBudget::new(session.deadline_unix_ms, caps.max_result_rows);
    let seen_budget = Arc::clone(&budget);
    let result = with_exec_budget(Arc::clone(&budget), || {
        nested_savepoint(txn, || {
            execute_typed_join_body(txn, describe, req, timing_source, caps, session)
        })
    })
    .await;
    record_query_rows_seen(seen_budget.rows_seen());
    result
}

/// Savepoint name for one nested `executeAtomic` on an open transaction.
const NESTED_ATOMIC_SAVEPOINT: &str = "bookclerk_nested_atomic";

/// Runs `f` inside `SAVEPOINT bookclerk_nested_atomic` and rolls back to it
/// on any error so a later outer commit cannot persist a partial batch.
///
/// # Errors
///
/// Returns [`DbErr`] when savepoint setup, `f`, or savepoint release fails.
async fn nested_savepoint<F, Fut, T>(txn: &DatabaseTransaction, f: F) -> Result<T, DbErr>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, DbErr>>,
{
    let backend = ConnectionTrait::get_database_backend(txn);
    txn.execute_raw(Statement::from_string(
        backend,
        format!("SAVEPOINT {NESTED_ATOMIC_SAVEPOINT}"),
    ))
    .await?;
    match f().await {
        Ok(value) => {
            if consume_savepoint_release_injection() {
                let msg = "database savepoint RELEASE failed: injected savepoint RELEASE failure";
                note_commit_failed(msg);
                return Err(DbErr::Custom(msg.into()));
            }
            if let Err(err) = txn
                .execute_raw(Statement::from_string(
                    backend,
                    format!("RELEASE SAVEPOINT {NESTED_ATOMIC_SAVEPOINT}"),
                ))
                .await
            {
                note_commit_failed(format!("database savepoint RELEASE failed: {err}"));
                return Err(err);
            }
            Ok(value)
        }
        Err(err) => {
            let rollback_err = if consume_savepoint_rollback_injection() {
                Some("injected savepoint ROLLBACK failure".to_string())
            } else {
                txn.execute_raw(Statement::from_string(
                    backend,
                    format!("ROLLBACK TO SAVEPOINT {NESTED_ATOMIC_SAVEPOINT}"),
                ))
                .await
                .err()
                .map(|e| e.to_string())
            };
            let release_err = if consume_savepoint_release_injection() {
                Some("injected savepoint RELEASE failure".to_string())
            } else {
                txn.execute_raw(Statement::from_string(
                    backend,
                    format!("RELEASE SAVEPOINT {NESTED_ATOMIC_SAVEPOINT}"),
                ))
                .await
                .err()
                .map(|e| e.to_string())
            };
            if rollback_err.is_none() && release_err.is_none() {
                return Err(err);
            }
            let cleanup = [rollback_err.as_deref(), release_err.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("; ");
            note_commit_failed(format!(
                "database savepoint cleanup failed after inner error: {cleanup}"
            ));
            Err(DbErr::Custom(format!("{err}; {cleanup}")))
        }
    }
}

/// Statement loop for [`execute_typed_on_txn`] (no COMMIT / ROLLBACK).
///
/// # Errors
///
/// Returns [`DbErr`] when a statement fails, encoding fails, or a result budget is exceeded.
async fn execute_typed_join_body(
    txn: &DatabaseTransaction,
    describe: Option<&DatabaseConnection>,
    req: &ExecuteRequest,
    timing_source: &str,
    caps: ExecCaps,
    session: AtomicSession,
) -> Result<ExecuteReply, DbErr> {
    let started = Instant::now();
    let backend = ConnectionTrait::get_database_backend(txn);
    let sql_started = Instant::now();
    let mut statements = Vec::with_capacity(req.statements.len());
    for stmt in &req.statements {
        session.check(AtomicInterruptPhase::BetweenStatements)?;
        let values: Vec<SeaValue> = stmt.parameters.iter().map(db_value_to_sea).collect();
        let row_cap = effective_row_cap(stmt.max_rows, caps.max_result_rows);
        let sql = if stmt.kind.wrap_select_limit() {
            cap_query_sql(&stmt.sql, row_cap)
        } else {
            stmt.sql.clone()
        };
        let sql = lower_canonical_sql(backend, &sql);
        let sea_stmt = Statement::from_sql_and_values(backend, &sql, values);
        let stmt_result = match stmt.result_selection {
            DbResultSelection::Rows => {
                if matches!(stmt.kind, DbPlanStatementKind::Execute) {
                    let exec = txn.execute_raw(sea_stmt).await?;
                    let result = StatementResult::from_affected(exec.rows_affected());
                    reject_statement_result_bytes(&result, caps.max_result_bytes)?;
                    result
                } else {
                    let engine_rows = collect_capped_query_results(txn, sea_stmt, row_cap).await?;
                    if engine_rows.is_empty()
                        && backend == sea_orm::DatabaseBackend::Postgres
                        && stmt.kind.wrap_select_limit()
                    {
                        if let Some(db) = describe {
                            record_postgres_empty_result_columns(db, &sql).await?;
                        }
                    }
                    statement_result_from_query_results(&engine_rows, stmt.kind, caps, row_cap)?
                }
            }
            DbResultSelection::AffectedRows | DbResultSelection::Discard => {
                let exec = txn.execute_raw(sea_stmt).await?;
                if matches!(stmt.result_selection, DbResultSelection::Discard) {
                    StatementResult::from_affected(0)
                } else {
                    let result = StatementResult::from_affected(exec.rows_affected());
                    reject_statement_result_bytes(&result, caps.max_result_bytes)?;
                    result
                }
            }
        };
        statements.push(stmt_result);
    }
    session.check(AtomicInterruptPhase::AroundCommit)?;
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
    reply.validate_positional().map_err(DbErr::Custom)?;
    match encoded_execute_reply_bytes(&reply) {
        Ok(bytes) => {
            if caps.max_atomic_result_bytes > 0 {
                let used = bytes.len();
                let cap = usize::try_from(caps.max_atomic_result_bytes).unwrap_or(usize::MAX);
                if used > cap {
                    return Err(DbErr::Custom(format!(
                        "atomic result is {used} bytes; maxAtomicResultBytes is {}",
                        caps.max_atomic_result_bytes
                    )));
                }
            }
        }
        Err(err) => {
            return Err(DbErr::Custom(format!(
                "failed to encode ExecuteReply on open transaction: {err}"
            )));
        }
    }
    Ok(reply)
}

/// Transaction body for [`execute_typed_on_session`]: run, encode, then COMMIT.
///
/// # Errors
///
/// Returns [`DbErr`] when a statement fails, encoding fails, or COMMIT fails.
async fn execute_typed_body<F>(
    db: &DatabaseConnection,
    req: &ExecuteRequest,
    timing_source: &str,
    caps: ExecCaps,
    session: AtomicSession,
    then: Option<F>,
) -> Result<ExecuteReply, DbErr>
where
    F: FnOnce(ExecuteReply) -> Result<Vec<TypedDbStatement>, DbErr>,
{
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
    // Host schema batches travel canonical; this adapter edge lowers/splits
    // them for the live backend and collapses the results back to the wire
    // request shape below.
    let wire_len = req.statements.len();
    let req = expand_host_schema_execute_request(backend, req);
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
        let row_cap = effective_row_cap(stmt.max_rows, caps.max_result_rows);
        let sql = if stmt.kind.wrap_select_limit() {
            cap_query_sql(&stmt.sql, row_cap)
        } else {
            stmt.sql.clone()
        };
        let sql = lower_canonical_sql(backend, &sql);
        let sea_stmt = Statement::from_sql_and_values(backend, &sql, values);
        let stmt_result = match stmt.result_selection {
            DbResultSelection::Rows => {
                if matches!(stmt.kind, DbPlanStatementKind::Execute) {
                    let exec = match txn.execute_raw(sea_stmt).await {
                        Ok(exec) => exec,
                        Err(err) => {
                            let _ = txn.rollback().await;
                            let _ = take_txn_fault();
                            return Err(err);
                        }
                    };
                    let result = StatementResult::from_affected(exec.rows_affected());
                    if let Err(err) = reject_statement_result_bytes(&result, caps.max_result_bytes)
                    {
                        let _ = txn.rollback().await;
                        let _ = take_txn_fault();
                        return Err(err);
                    }
                    result
                } else {
                    let engine_rows =
                        match collect_capped_query_results(&txn, sea_stmt, row_cap).await {
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
                    match statement_result_from_query_results(
                        &engine_rows,
                        stmt.kind,
                        caps,
                        row_cap,
                    ) {
                        Ok(result) => result,
                        Err(err) => {
                            let _ = txn.rollback().await;
                            let _ = take_txn_fault();
                            return Err(err);
                        }
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
                    if let Err(err) = reject_statement_result_bytes(&result, caps.max_result_bytes)
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
    let statements = crate::schema_postgres::collapse_host_schema_results(wire_len, statements);
    if let Some(then) = then {
        let partial = ExecuteReply {
            operation_id: req.operation_id.clone(),
            statements: statements.clone(),
            timing: DbTiming {
                attempt_elapsed_us: u64::try_from(started.elapsed().as_micros())
                    .unwrap_or(u64::MAX),
                db_execution_us: u64::try_from(sql_started.elapsed().as_micros())
                    .unwrap_or(u64::MAX),
                db_timing_source: timing_source.to_string(),
            },
        };
        for stmt in then(partial)? {
            if let Err(err) = session.check(AtomicInterruptPhase::BetweenStatements) {
                let _ = txn.rollback().await;
                let _ = take_txn_fault();
                return Err(err);
            }
            let values: Vec<SeaValue> = stmt.parameters.iter().map(db_value_to_sea).collect();
            let sql = lower_canonical_sql(backend, &stmt.sql);
            let sea_stmt = Statement::from_sql_and_values(backend, &sql, values);
            match txn.execute_raw(sea_stmt).await {
                Ok(_) => {}
                Err(err) => {
                    let _ = txn.rollback().await;
                    let _ = take_txn_fault();
                    return Err(err);
                }
            }
        }
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
