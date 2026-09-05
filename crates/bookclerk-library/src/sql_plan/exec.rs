//! Run a typed [`ExecuteRequest`] on a SeaORM connection (one native transaction).
//!
//! This is the **in-process adapter** entry: callers must pass the
//! [`bookclerk_db_exec::PhysicalEngine`] they opened. Host RPC/proxy paths
//! must not call these helpers — they stamp [`AdapterExecuteRequest`] and
//! send it to [`crate::TypedAtomicExec`].

use bookclerk_db_exec::{db_value_from_sea, ExecCaps, PhysicalEngine};
use bookclerk_plugin_abi::{
    DbPlanStatementKind, DbResultSelection, DbRow, ExecuteReply, ExecuteRequest, SqlTypeEnv,
    TypedDbStatement,
};
use sea_orm::{ConnectionTrait, StreamTrait, Value};

use crate::atomic_ops::DbAtomicResult;
use crate::error::{LibraryError, Result};
use crate::sql_plan::interpret::interpret_typed_exec;
use crate::sql_plan::CompiledAtomic;

pub use bookclerk_db_exec::AtomicSession;

/// Stamp and run one canonical statement on an already-open in-process connection.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when typecheck, lowering, or execute fails.
pub(crate) async fn execute_typed_on_open<C>(
    engine: PhysicalEngine,
    conn: &C,
    req: &ExecuteRequest,
    type_env: SqlTypeEnv,
    max_result_rows: u32,
) -> Result<ExecuteReply>
where
    C: ConnectionTrait + StreamTrait,
{
    let envelope = bookclerk_db_exec::stamp_adapter_execute(req.clone(), &type_env)
        .map_err(LibraryError::from_db_err)?;
    bookclerk_db_exec::execute_typed_on_open_envelope(
        engine,
        conn,
        &envelope,
        engine.timing_source(),
        ExecCaps::from(max_result_rows),
        AtomicSession::from_deadline(None).with_type_env(type_env),
        None,
    )
    .await
    .map_err(LibraryError::from_db_err)
}

/// Execute leftover SQL: canonical transport, or one physical lowering pass.
///
/// # Errors
///
/// Returns when the connection rejects the statement.
pub(crate) async fn execute_sql_on<C>(
    engine: Option<PhysicalEngine>,
    conn: &C,
    sql: &str,
    values: impl IntoIterator<Item = Value>,
    type_env: SqlTypeEnv,
) -> Result<u64>
where
    C: ConnectionTrait + StreamTrait,
{
    let values: Vec<Value> = values.into_iter().collect();
    if let Some(engine) = engine {
        let parameters = values
            .iter()
            .map(db_value_from_sea)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| LibraryError::from_db_err(sea_orm::DbErr::Custom(err)))?;
        let req = ExecuteRequest {
            operation_id: "in-process-sql".into(),
            request_hash: String::new(),
            deadline_unix_ms: 0,
            statements: vec![TypedDbStatement {
                sql: sql.to_string(),
                parameters,
                kind: DbPlanStatementKind::Execute,
                max_rows: 0,
                result_selection: DbResultSelection::AffectedRows,
            }],
        };
        let reply = execute_typed_on_open(engine, conn, &req, type_env, 0).await?;
        return Ok(reply
            .statements
            .first()
            .map(|stmt| stmt.rows_affected)
            .unwrap_or(0));
    }
    let res = crate::host_sql::execute_host_canonical(conn, sql, values)
        .await
        .map_err(LibraryError::from_db_err)?;
    Ok(res.rows_affected())
}

/// Query leftover SQL on a real in-process engine (one physical lowering pass).
///
/// # Errors
///
/// Returns when typecheck, lowering, or execute fails.
pub(crate) async fn query_sql_on<C>(
    engine: Option<PhysicalEngine>,
    conn: &C,
    sql: &str,
    values: impl IntoIterator<Item = Value>,
    type_env: &SqlTypeEnv,
    max_rows: u32,
) -> Result<Vec<DbRow>>
where
    C: ConnectionTrait + StreamTrait,
{
    let Some(engine) = engine else {
        return Err(LibraryError::Schema(
            "query_sql_on requires a physical in-process engine".into(),
        ));
    };
    let parameters = values
        .into_iter()
        .map(|value| db_value_from_sea(&value))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|err| LibraryError::from_db_err(sea_orm::DbErr::Custom(err)))?;
    let req = ExecuteRequest {
        operation_id: "in-process-query".into(),
        request_hash: String::new(),
        deadline_unix_ms: 0,
        statements: vec![TypedDbStatement {
            sql: sql.to_string(),
            parameters,
            kind: DbPlanStatementKind::Select,
            max_rows,
            result_selection: DbResultSelection::Rows,
        }],
    };
    let reply = execute_typed_on_open(engine, conn, &req, type_env.clone(), max_rows).await?;
    Ok(reply
        .statements
        .into_iter()
        .next()
        .map(|stmt| stmt.rows)
        .unwrap_or_default())
}

/// Executes a compiled named atomic as one transaction and interprets the reply.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when a statement fails. Application statuses
/// are returned as [`DbAtomicResult`], not errors.
pub async fn execute_compiled_on(
    engine: PhysicalEngine,
    db: &sea_orm::DatabaseConnection,
    compiled: CompiledAtomic,
    timing_source: &str,
) -> Result<DbAtomicResult> {
    execute_compiled_on_capped(engine, db, compiled, timing_source, 0).await
}

/// Like [`execute_compiled_on`], failing when a statement returns more than `max_result_rows`.
///
/// `max_result_rows` of `0` means unlimited.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when a statement fails or exceeds the row cap.
pub async fn execute_compiled_on_capped(
    engine: PhysicalEngine,
    db: &sea_orm::DatabaseConnection,
    compiled: CompiledAtomic,
    timing_source: &str,
    max_result_rows: u32,
) -> Result<DbAtomicResult> {
    let hash = compiled.expected_hash.clone();
    let reply = execute_typed_on(
        engine,
        db,
        &compiled.request,
        timing_source,
        max_result_rows,
    )
    .await?;
    Ok(interpret_typed_exec(&compiled, &reply, &hash))
}

/// Executes a typed request as one transaction on a known physical engine.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when a statement fails.
pub async fn execute_typed_on(
    engine: PhysicalEngine,
    db: &sea_orm::DatabaseConnection,
    req: &ExecuteRequest,
    timing_source: &str,
    max_result_rows: u32,
) -> Result<bookclerk_plugin_abi::ExecuteReply> {
    execute_typed_on_session(
        engine,
        db,
        req,
        timing_source,
        max_result_rows,
        AtomicSession::default(),
    )
    .await
}

/// [`execute_typed_on`] with session cancel / deadline checks.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when a statement fails or the session is interrupted.
pub async fn execute_typed_on_session(
    engine: PhysicalEngine,
    db: &sea_orm::DatabaseConnection,
    req: &ExecuteRequest,
    timing_source: &str,
    max_result_rows: u32,
    session: AtomicSession,
) -> Result<bookclerk_plugin_abi::ExecuteReply> {
    let type_env = crate::migrations::host_sql_type_env();
    let envelope = bookclerk_db_exec::stamp_adapter_execute(req.clone(), &type_env)
        .map_err(LibraryError::from_db_err)?;
    bookclerk_db_exec::execute_typed_envelope(
        engine,
        db,
        &envelope,
        timing_source,
        ExecCaps::from(max_result_rows),
        session.with_type_env(type_env),
    )
    .await
    .map_err(LibraryError::from_db_err)
}
