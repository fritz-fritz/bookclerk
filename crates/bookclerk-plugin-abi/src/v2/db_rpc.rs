//! Cap'n Proto codec for typed database execute / capabilities.

#![allow(clippy::missing_docs_in_private_items)]

use super::plugin_v2_capnp::{
    db_capabilities as db_caps_capnp, db_capabilities_reply, db_column as db_column_capnp,
    db_row as db_row_capnp, db_statement as db_statement_capnp, db_timing as db_timing_capnp,
    db_value as db_value_capnp, execute_reply as execute_reply_capnp,
    execute_request as execute_request_capnp, execute_result_reply,
    statement_result as statement_result_capnp, DbResultSelection as CapnpDbResultSelection,
    DbStatementKind as CapnpDbStatementKind, DbType as CapnpDbType,
};
use super::rpc::{from_capnp, read_error, text_of, write_error};
use crate::{
    DbCapabilities, DbColumn, DbPlanStatementKind, DbResultSelection, DbRow, DbTiming, DbType,
    DbValue, ExecuteReply, ExecuteRequest, PluginError, Result, StatementResult, TypedDbStatement,
};

pub(super) fn write_db_type(ty: DbType) -> CapnpDbType {
    match ty {
        DbType::Unspecified => CapnpDbType::Unspecified,
        DbType::Bool => CapnpDbType::Bool,
        DbType::Int64 => CapnpDbType::Int64,
        DbType::Float64 => CapnpDbType::Float64,
        DbType::Text => CapnpDbType::Text,
        DbType::Bytes => CapnpDbType::Bytes,
    }
}

/// # Errors
///
/// Returns [`PluginError::unsupported`] when the wire tag is unknown.
pub(super) fn read_db_type(ty: CapnpDbType) -> Result<DbType> {
    match ty {
        CapnpDbType::Unspecified => Ok(DbType::Unspecified),
        CapnpDbType::Bool => Ok(DbType::Bool),
        CapnpDbType::Int64 => Ok(DbType::Int64),
        CapnpDbType::Float64 => Ok(DbType::Float64),
        CapnpDbType::Text => Ok(DbType::Text),
        CapnpDbType::Bytes => Ok(DbType::Bytes),
    }
}

pub(super) fn write_db_value(mut b: db_value_capnp::Builder<'_>, v: &DbValue) {
    match v {
        DbValue::Null(ty) => b.set_null(write_db_type(*ty)),
        DbValue::Boolean(x) => b.set_boolean(*x),
        DbValue::Int64(x) => b.set_int64(*x),
        DbValue::Float64(x) => b.set_float64(*x),
        DbValue::Text(s) => b.set_text(s),
        DbValue::Bytes(d) => b.set_bytes(d),
    }
}

/// # Errors
///
/// Returns [`PluginError::unsupported`] or [`PluginError::invalid_params`] when
/// the union member is unknown or a float64 is not finite.
pub(super) fn read_db_value(r: db_value_capnp::Reader<'_>) -> Result<DbValue> {
    match r.which() {
        Ok(db_value_capnp::Null(ty)) => {
            let ty = ty.map_err(|_| PluginError::unsupported("unknown DbType"))?;
            Ok(DbValue::Null(read_db_type(ty)?))
        }
        Ok(db_value_capnp::Boolean(x)) => Ok(DbValue::Boolean(x)),
        Ok(db_value_capnp::Int64(x)) => Ok(DbValue::Int64(x)),
        Ok(db_value_capnp::Float64(x)) => {
            if !x.is_finite() {
                return Err(PluginError::invalid_params("float64 value is not finite"));
            }
            Ok(DbValue::Float64(x))
        }
        Ok(db_value_capnp::Text(t)) => Ok(DbValue::Text(text_of(t.map_err(from_capnp)?))),
        Ok(db_value_capnp::Bytes(d)) => Ok(DbValue::Bytes(d.map_err(from_capnp)?.to_vec())),
        Err(_) => Err(PluginError::unsupported("unknown DbValue union member")),
    }
}

fn write_kind(kind: DbPlanStatementKind) -> CapnpDbStatementKind {
    match kind {
        DbPlanStatementKind::Query => CapnpDbStatementKind::Query,
        DbPlanStatementKind::Execute => CapnpDbStatementKind::Execute,
        DbPlanStatementKind::Select => CapnpDbStatementKind::Select,
        DbPlanStatementKind::Returning => CapnpDbStatementKind::Returning,
    }
}

/// # Errors
///
/// Returns [`PluginError::unsupported`] when the wire tag is unknown.
fn read_kind(kind: CapnpDbStatementKind) -> Result<DbPlanStatementKind> {
    match kind {
        CapnpDbStatementKind::Query => Ok(DbPlanStatementKind::Query),
        CapnpDbStatementKind::Execute => Ok(DbPlanStatementKind::Execute),
        CapnpDbStatementKind::Select => Ok(DbPlanStatementKind::Select),
        CapnpDbStatementKind::Returning => Ok(DbPlanStatementKind::Returning),
    }
}

fn write_selection(sel: DbResultSelection) -> CapnpDbResultSelection {
    match sel {
        DbResultSelection::Discard => CapnpDbResultSelection::Discard,
        DbResultSelection::AffectedRows => CapnpDbResultSelection::AffectedRows,
        DbResultSelection::Rows => CapnpDbResultSelection::Rows,
    }
}

/// # Errors
///
/// Returns [`PluginError::unsupported`] when the wire tag is unknown.
fn read_selection(sel: CapnpDbResultSelection) -> Result<DbResultSelection> {
    match sel {
        CapnpDbResultSelection::Discard => Ok(DbResultSelection::Discard),
        CapnpDbResultSelection::AffectedRows => Ok(DbResultSelection::AffectedRows),
        CapnpDbResultSelection::Rows | CapnpDbResultSelection::ObsoleteCursor => {
            Ok(DbResultSelection::Rows)
        }
    }
}

fn write_db_statement(mut b: db_statement_capnp::Builder<'_>, stmt: &TypedDbStatement) {
    b.set_sql(&stmt.sql);
    b.set_kind(write_kind(stmt.kind));
    b.set_max_rows(stmt.max_rows);
    b.set_result_selection(write_selection(stmt.result_selection));
    let mut params = b.reborrow().init_parameters(stmt.parameters.len() as u32);
    for (i, p) in stmt.parameters.iter().enumerate() {
        write_db_value(params.reborrow().get(i as u32), p);
    }
}

/// # Errors
///
/// Returns when SQL, parameters, or statement tags cannot be decoded.
fn read_db_statement(r: db_statement_capnp::Reader<'_>) -> Result<TypedDbStatement> {
    let sql = text_of(r.get_sql().map_err(from_capnp)?);
    let kind = read_kind(
        r.get_kind()
            .map_err(|_| PluginError::unsupported("unknown DbStatementKind"))?,
    )?;
    let max_rows = r.get_max_rows();
    let result_selection = match r.get_result_selection() {
        Ok(sel) => read_selection(sel).unwrap_or(DbResultSelection::Rows),
        Err(_) => DbResultSelection::Rows,
    };
    let list = r.get_parameters().map_err(from_capnp)?;
    let mut parameters = Vec::with_capacity(list.len() as usize);
    for item in list.iter() {
        parameters.push(read_db_value(item)?);
    }
    Ok(TypedDbStatement {
        sql,
        parameters,
        kind,
        max_rows,
        result_selection,
    })
}

pub(super) fn write_execute_request(
    mut b: execute_request_capnp::Builder<'_>,
    req: &ExecuteRequest,
) {
    b.set_operation_id(&req.operation_id);
    b.set_request_hash(&req.request_hash);
    b.set_deadline_unix_ms(req.deadline_unix_ms);
    let mut stmts = b.reborrow().init_statements(req.statements.len() as u32);
    for (i, s) in req.statements.iter().enumerate() {
        write_db_statement(stmts.reborrow().get(i as u32), s);
    }
}

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the statement list is empty, or
/// [`PluginError::unsupported`] when a nested field cannot be decoded.
pub(super) fn read_execute_request(r: execute_request_capnp::Reader<'_>) -> Result<ExecuteRequest> {
    let list = r.get_statements().map_err(from_capnp)?;
    let mut statements = Vec::with_capacity(list.len() as usize);
    for item in list.iter() {
        statements.push(read_db_statement(item)?);
    }
    if statements.is_empty() {
        return Err(PluginError::invalid_params(
            "executeAtomic statements must be non-empty",
        ));
    }
    Ok(ExecuteRequest {
        operation_id: text_of(r.get_operation_id().map_err(from_capnp)?),
        request_hash: text_of(r.get_request_hash().map_err(from_capnp)?),
        statements,
        deadline_unix_ms: r.get_deadline_unix_ms(),
    })
}

fn write_column(mut b: db_column_capnp::Builder<'_>, col: &DbColumn) {
    b.set_name(&col.name);
    b.set_db_type(write_db_type(col.db_type));
}

/// # Errors
///
/// Returns when the column name or declared type cannot be decoded.
fn read_column(r: db_column_capnp::Reader<'_>) -> Result<DbColumn> {
    Ok(DbColumn {
        name: text_of(r.get_name().map_err(from_capnp)?),
        db_type: read_db_type(
            r.get_db_type()
                .map_err(|_| PluginError::unsupported("unknown DbType"))?,
        )?,
    })
}

fn write_row(mut b: db_row_capnp::Builder<'_>, row: &DbRow) {
    let mut vals = b.reborrow().init_values(row.values.len() as u32);
    for (i, v) in row.values.iter().enumerate() {
        write_db_value(vals.reborrow().get(i as u32), v);
    }
}

/// # Errors
///
/// Returns when a cell cannot be decoded as [`DbValue`].
fn read_row(r: db_row_capnp::Reader<'_>) -> Result<DbRow> {
    let list = r.get_values().map_err(from_capnp)?;
    let mut values = Vec::with_capacity(list.len() as usize);
    for item in list.iter() {
        values.push(read_db_value(item)?);
    }
    Ok(DbRow { values })
}

fn write_statement_result(mut b: statement_result_capnp::Builder<'_>, stmt: &StatementResult) {
    b.set_rows_affected(stmt.rows_affected);
    let mut cols = b.reborrow().init_columns(stmt.columns.len() as u32);
    for (i, c) in stmt.columns.iter().enumerate() {
        write_column(cols.reborrow().get(i as u32), c);
    }
    let mut rows = b.reborrow().init_rows(stmt.rows.len() as u32);
    for (i, row) in stmt.rows.iter().enumerate() {
        write_row(rows.reborrow().get(i as u32), row);
    }
}

/// # Errors
///
/// Returns when columns/rows cannot be decoded, a row width does not match
/// `columns.len()`, or column names are duplicated.
fn read_statement_result(r: statement_result_capnp::Reader<'_>) -> Result<StatementResult> {
    let cols = r.get_columns().map_err(from_capnp)?;
    let mut columns = Vec::with_capacity(cols.len() as usize);
    for item in cols.iter() {
        columns.push(read_column(item)?);
    }
    let rows_list = r.get_rows().map_err(from_capnp)?;
    let mut rows = Vec::with_capacity(rows_list.len() as usize);
    for item in rows_list.iter() {
        rows.push(read_row(item)?);
    }
    let result = StatementResult {
        rows,
        columns,
        rows_affected: r.get_rows_affected(),
    };
    result
        .validate_positional()
        .map_err(PluginError::invalid_params)?;
    Ok(result)
}

fn write_timing(mut b: db_timing_capnp::Builder<'_>, t: &DbTiming) {
    b.set_attempt_elapsed_us(t.attempt_elapsed_us);
    b.set_db_execution_us(t.db_execution_us);
    b.set_db_timing_source(&t.db_timing_source);
}

/// # Errors
///
/// Returns when the timing source string cannot be decoded.
fn read_timing(r: db_timing_capnp::Reader<'_>) -> Result<DbTiming> {
    Ok(DbTiming {
        attempt_elapsed_us: r.get_attempt_elapsed_us(),
        db_execution_us: r.get_db_execution_us(),
        db_timing_source: text_of(r.get_db_timing_source().map_err(from_capnp)?),
    })
}

pub(super) fn fill_execute_reply(mut b: execute_reply_capnp::Builder<'_>, reply: &ExecuteReply) {
    b.set_operation_id(&reply.operation_id);
    write_timing(b.reborrow().init_timing(), &reply.timing);
    let mut stmts = b.reborrow().init_statements(reply.statements.len() as u32);
    for (i, s) in reply.statements.iter().enumerate() {
        write_statement_result(stmts.reborrow().get(i as u32), s);
    }
}

/// # Errors
///
/// Returns when statements or timing cannot be decoded, or a statement result
/// fails positional validation.
pub(super) fn read_execute_reply(r: execute_reply_capnp::Reader<'_>) -> Result<ExecuteReply> {
    let list = r.get_statements().map_err(from_capnp)?;
    let mut statements = Vec::with_capacity(list.len() as usize);
    for item in list.iter() {
        statements.push(read_statement_result(item)?);
    }
    let timing = r
        .get_timing()
        .ok()
        .map(read_timing)
        .transpose()?
        .unwrap_or_default();
    Ok(ExecuteReply {
        operation_id: text_of(r.get_operation_id().map_err(from_capnp)?),
        statements,
        timing,
    })
}

pub(super) fn write_execute_result_reply(
    result: execute_result_reply::Builder<'_>,
    outcome: Result<ExecuteReply>,
) {
    match outcome {
        Ok(reply) => fill_execute_reply(result.init_ok(), &reply),
        Err(err) => write_error(result.init_err(), &err),
    }
}

/// # Errors
///
/// Returns the guest [`PluginError`] on `err`, or a decode failure on `ok`.
pub(super) fn read_execute_result_reply(
    result: execute_result_reply::Reader<'_>,
) -> Result<ExecuteReply> {
    match result.which().map_err(from_capnp)? {
        execute_result_reply::Ok(ok) => read_execute_reply(ok.map_err(from_capnp)?),
        execute_result_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
    }
}

pub(super) fn write_db_capabilities(mut b: db_caps_capnp::Builder<'_>, caps: &DbCapabilities) {
    b.set_sql_contract_version(caps.sql_contract_version);
    b.set_atomic_batch(caps.atomic_batch);
    b.set_returning(caps.returning);
    b.set_affected_rows(caps.affected_rows);
    b.set_schema_migrations(caps.schema_migrations);
    b.set_pragma_user_version(caps.pragma_user_version);
    b.set_atomic_schema_batch(caps.atomic_schema_batch);
    b.set_cancellation(caps.cancellation);
    b.set_timing(caps.timing);
    b.set_max_binds(caps.max_binds);
    b.set_max_statements(caps.max_statements);
    b.set_max_result_rows(caps.max_result_rows);
    b.set_max_payload_bytes(caps.max_payload_bytes);
    b.set_max_result_bytes(caps.max_result_bytes);
    b.set_max_cell_bytes(caps.max_cell_bytes);
    b.set_max_request_bytes(caps.max_request_bytes);
    b.set_max_atomic_result_bytes(caps.max_atomic_result_bytes);
}

/// # Errors
///
/// Returns when obsolete bootstrap tombstones cannot be decoded (ignored when empty).
pub(super) fn read_db_capabilities(r: db_caps_capnp::Reader<'_>) -> Result<DbCapabilities> {
    let _ = r.get_obsolete_diagnostic_engine().map_err(from_capnp)?;
    let _ = r.get_obsolete_sql_family().map_err(from_capnp)?;
    Ok(DbCapabilities {
        sql_contract_version: r.get_sql_contract_version(),
        atomic_batch: r.get_atomic_batch(),
        returning: r.get_returning(),
        affected_rows: r.get_affected_rows(),
        schema_migrations: r.get_schema_migrations(),
        pragma_user_version: r.get_pragma_user_version(),
        atomic_schema_batch: r.get_atomic_schema_batch(),
        cancellation: r.get_cancellation(),
        timing: r.get_timing(),
        max_binds: r.get_max_binds(),
        max_statements: r.get_max_statements(),
        max_result_rows: r.get_max_result_rows(),
        max_payload_bytes: r.get_max_payload_bytes(),
        max_result_bytes: r.get_max_result_bytes(),
        max_cell_bytes: r.get_max_cell_bytes(),
        max_request_bytes: r.get_max_request_bytes(),
        max_atomic_result_bytes: r.get_max_atomic_result_bytes(),
    })
}

pub(super) fn write_db_capabilities_reply(
    result: db_capabilities_reply::Builder<'_>,
    outcome: Result<DbCapabilities>,
) {
    match outcome {
        Ok(caps) => write_db_capabilities(result.init_ok(), &caps),
        Err(err) => write_error(result.init_err(), &err),
    }
}

/// # Errors
///
/// Returns the guest [`PluginError`] on `err`, or a decode failure on `ok`.
pub(super) fn read_db_capabilities_reply(
    result: db_capabilities_reply::Reader<'_>,
) -> Result<DbCapabilities> {
    match result.which().map_err(from_capnp)? {
        db_capabilities_reply::Ok(ok) => read_db_capabilities(ok.map_err(from_capnp)?),
        db_capabilities_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
    }
}

/// Serializes a Cap'n message to unpacked stream bytes.
///
/// # Errors
///
/// Returns [`PluginError::internal`] when serialization fails.
fn write_message_bytes(
    message: &capnp::message::Builder<capnp::message::HeapAllocator>,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    capnp::serialize::write_message(&mut out, message).map_err(from_capnp)?;
    Ok(out)
}

/// Encodes a standalone Cap'n `DbValue` message (unpacked stream).
///
/// # Errors
///
/// Returns [`PluginError::internal`] when the message cannot be serialized.
pub fn encoded_db_value_bytes(v: &DbValue) -> Result<Vec<u8>> {
    let mut message = capnp::message::Builder::new_default();
    write_db_value(message.init_root(), v);
    write_message_bytes(&message)
}

/// Encodes a standalone Cap'n `ExecuteRequest` message (unpacked stream).
///
/// # Errors
///
/// Returns [`PluginError::internal`] when the message cannot be serialized.
pub fn encoded_execute_request_bytes(req: &ExecuteRequest) -> Result<Vec<u8>> {
    let mut message = capnp::message::Builder::new_default();
    write_execute_request(message.init_root(), req);
    write_message_bytes(&message)
}

/// SHA-256 hex of the Cap'n-encoded request with transport metadata cleared.
///
/// `operationId`, `requestHash`, and `deadlineUnixMs` are not part of the
/// digest: a retry of the same mutation may mint a remaining deadline without
/// hash-conflicting. The trusted host stamps `requestHash` after validation
/// so guests cannot collide unrelated mutations on one idempotency key.
///
/// # Errors
///
/// Returns [`PluginError::internal`] when the message cannot be serialized.
pub fn canonical_execute_request_hash(req: &ExecuteRequest) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut canonical = req.clone();
    canonical.operation_id.clear();
    canonical.request_hash.clear();
    canonical.deadline_unix_ms = 0;
    let bytes = encoded_execute_request_bytes(&canonical)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

/// Encodes a standalone Cap'n `ExecuteReply` message (unpacked stream).
///
/// # Errors
///
/// Returns [`PluginError::internal`] when the message cannot be serialized.
pub fn encoded_execute_reply_bytes(reply: &ExecuteReply) -> Result<Vec<u8>> {
    let mut message = capnp::message::Builder::new_default();
    fill_execute_reply(message.init_root(), reply);
    write_message_bytes(&message)
}

/// Encodes a standalone Cap'n `ExecuteAtomicReply` (`ok` or `err`).
///
/// # Errors
///
/// Returns [`PluginError::internal`] when the message cannot be serialized.
pub fn encoded_execute_result_reply_bytes(outcome: Result<ExecuteReply>) -> Result<Vec<u8>> {
    let mut message = capnp::message::Builder::new_default();
    write_execute_result_reply(message.init_root(), outcome);
    write_message_bytes(&message)
}

/// Decodes a standalone Cap'n `ExecuteAtomicReply` message.
///
/// # Errors
///
/// Returns the guest [`PluginError`] on `err`, or a decode failure on `ok`.
pub fn decode_execute_result_reply_bytes(bytes: &[u8]) -> Result<ExecuteReply> {
    let mut cursor = std::io::Cursor::new(bytes);
    let reader = capnp::serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .map_err(from_capnp)?;
    read_execute_result_reply(reader.get_root().map_err(from_capnp)?)
}

/// Encodes a standalone Cap'n `StatementResult` message (unpacked stream).
///
/// Used to enforce negotiated `maxResultBytes` against the actual wire size of
/// one statement before COMMIT.
///
/// # Errors
///
/// Returns [`PluginError::internal`] when the message cannot be serialized.
pub fn encoded_statement_result_bytes(stmt: &StatementResult) -> Result<Vec<u8>> {
    let mut message = capnp::message::Builder::new_default();
    write_statement_result(message.init_root(), stmt);
    write_message_bytes(&message)
}

/// Decodes a standalone Cap'n `DbValue` message.
///
/// # Errors
///
/// Returns when the buffer is not a valid unpacked `DbValue`.
pub fn decode_db_value_bytes(bytes: &[u8]) -> Result<DbValue> {
    let mut cursor = std::io::Cursor::new(bytes);
    let reader = capnp::serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .map_err(from_capnp)?;
    read_db_value(reader.get_root().map_err(from_capnp)?)
}

/// Decodes a standalone Cap'n `ExecuteRequest` message.
///
/// # Errors
///
/// Returns when the buffer is not a valid unpacked `ExecuteRequest`.
pub fn decode_execute_request_bytes(bytes: &[u8]) -> Result<ExecuteRequest> {
    let mut cursor = std::io::Cursor::new(bytes);
    let reader = capnp::serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .map_err(from_capnp)?;
    read_execute_request(reader.get_root().map_err(from_capnp)?)
}
