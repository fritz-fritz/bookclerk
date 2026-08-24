//! Generic D1 HTTP `batch()` executor for host-authored SQL plans.
//!
//! The guest does not parse Bookclerk operation names. The host compiles
//! domain work into [`DbAtomicPlan`]; this module runs that list as one
//! D1 `{ "batch": [...] }` SQL transaction and returns statement results.

use std::time::Duration;

use bookclerk_db_exec::{
    sea_null_kind, DbAtomicPlan, DbAtomicRequest, DbAtomicTiming, DbPlanExecResult,
    DbPlanStatementKind, DbPlanStmtExecResult,
};
use bookclerk_plugin_sdk::{
    encoded_execute_reply_bytes, encoded_statement_result_bytes, DbColumn, DbConnectResult,
    DbResultSelection, DbRow, DbTiming, DbType, DbValue, ExecuteReply, ExecuteRequest, PluginError,
    StatementResult, TypedDbStatement,
};
use sea_orm::DbErr;
use serde_json::Value as JsonValue;

use super::d1::D1Proxy;

/// One statement in a D1 HTTP batch body.
pub(crate) type SqlStmt = (String, Vec<JsonValue>);

/// Maximum D1 HTTP batch attempts, including retries after ambiguous responses.
const ATOMIC_HTTP_ATTEMPTS: usize = 3;

impl D1Proxy {
    /// Runs a host-authored plan as one D1 HTTP batch (one SQL transaction).
    ///
    /// # Errors
    ///
    /// Returns an error when the plan is missing, HTTP fails, or the batch
    /// response is malformed.
    pub async fn run_atomic(
        &self,
        req: DbAtomicRequest,
    ) -> std::result::Result<DbPlanExecResult, DbErr> {
        let started = std::time::Instant::now();
        check_d1_session(
            bookclerk_db_exec::AtomicInterruptPhase::BeforeBegin,
            req.deadline_unix_ms,
        )?;
        let plan = req
            .plan
            .clone()
            .ok_or_else(|| DbErr::Custom("dbAtomic requires a host-authored executePlan".into()))?;
        reject_unbounded_returning(&plan)?;
        let d1_caps = DbConnectResult::d1();
        let cap = d1_caps.max_result_rows;
        let statements: Vec<SqlStmt> = plan
            .statements
            .iter()
            .map(|s| {
                let sql = if s.kind.wrap_select_limit() {
                    bookclerk_db_exec::cap_query_sql(&s.sql, cap)
                } else {
                    s.sql.clone()
                };
                (sql, d1_wire_binds(&s.binds))
            })
            .collect();
        let mut last_err = None;
        for attempt in 0..ATOMIC_HTTP_ATTEMPTS {
            check_d1_session(
                bookclerk_db_exec::AtomicInterruptPhase::BeforeBegin,
                req.deadline_unix_ms,
            )?;
            let timeout = d1_http_timeout(req.deadline_unix_ms)?;
            let raw = match self.run_batch_with_timeout(&statements, timeout).await {
                Ok(value) => {
                    check_d1_session(
                        bookclerk_db_exec::AtomicInterruptPhase::AroundCommit,
                        req.deadline_unix_ms,
                    )?;
                    value
                }
                Err(err) if err.is_retryable() && attempt + 1 < ATOMIC_HTTP_ATTEMPTS => {
                    sleep_before_d1_retry_bounded(attempt, err.retry_after(), req.deadline_unix_ms)
                        .await?;
                    last_err = Some(DbErr::from(err));
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
            match parse_generic_batch(&plan, &raw, req.operation_id.clone(), started) {
                Ok(result) => return Ok(result),
                Err(err) if is_ambiguous_d1(&err) && attempt + 1 < ATOMIC_HTTP_ATTEMPTS => {
                    sleep_before_d1_retry_bounded(attempt, None, req.deadline_unix_ms).await?;
                    last_err = Some(err);
                }
                Err(err) => return Err(err),
            }
        }
        Err(last_err.unwrap_or_else(|| ambiguous_d1("exhausted retries")))
    }

    /// Runs a typed [`ExecuteRequest`] as one D1 HTTP batch.
    ///
    /// [`DbValue::Text`] is sent as a JSON string even when it starts with
    /// `b64:`. [`DbValue::Bytes`] uses the D1 HTTP `b64:` adapter convention.
    /// After HTTP success, encode/size failures are ambiguous (`unavailable`).
    ///
    /// # Errors
    ///
    /// Returns when the batch is rejected, HTTP fails, or the reply cannot be
    /// encoded after commit.
    pub async fn run_typed_atomic(
        &self,
        req: &ExecuteRequest,
        guest_receipt: bookclerk_plugin_sdk::host_db::GuestReceiptPersist,
    ) -> std::result::Result<ExecuteReply, DbErr> {
        let started = std::time::Instant::now();
        let deadline = (req.deadline_unix_ms > 0).then_some(req.deadline_unix_ms);
        check_d1_session(
            bookclerk_db_exec::AtomicInterruptPhase::BeforeBegin,
            deadline,
        )?;
        reject_unbounded_returning_typed(&req.statements)?;
        let d1_caps = DbConnectResult::d1();
        let cap = d1_caps.max_result_rows;
        let statements: Vec<SqlStmt> = req
            .statements
            .iter()
            .map(|s| {
                let sql = if s.kind.wrap_select_limit() {
                    bookclerk_db_exec::cap_query_sql(&s.sql, cap)
                } else {
                    s.sql.clone()
                };
                (sql, d1_typed_binds(&s.parameters))
            })
            .collect();
        let mut last_err = None;
        for attempt in 0..ATOMIC_HTTP_ATTEMPTS {
            check_d1_session(
                bookclerk_db_exec::AtomicInterruptPhase::BeforeBegin,
                deadline,
            )?;
            let timeout = d1_http_timeout(deadline)?;
            let raw = match self.run_batch_with_timeout(&statements, timeout).await {
                Ok(value) => {
                    check_d1_session(
                        bookclerk_db_exec::AtomicInterruptPhase::AroundCommit,
                        deadline,
                    )?;
                    value
                }
                Err(err) if err.is_retryable() && attempt + 1 < ATOMIC_HTTP_ATTEMPTS => {
                    sleep_before_d1_retry_bounded(attempt, err.retry_after(), deadline).await?;
                    last_err = Some(DbErr::from(err));
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
            match parse_typed_batch(req, &raw, started) {
                Ok(reply) => {
                    if !guest_receipt.is_absent() {
                        // Guest-receipt finalize needs statement results, so D1 runs a
                        // follow-up HTTP batch after the main batch commits. Same-batch
                        // finalize would require provider support for dependent SQL.
                        let hint = &guest_receipt;
                        let finalize = bookclerk_db_exec::guest_receipt_finalize_stmts(
                            &reply,
                            usize::try_from(hint.guest_statement_len).unwrap_or(usize::MAX),
                            &hint.guest_request_hash,
                        )?;
                        if !finalize.is_empty() {
                            let fin_stmts: Vec<SqlStmt> = finalize
                                .iter()
                                .map(|s| {
                                    let sql = if s.kind.wrap_select_limit() {
                                        bookclerk_db_exec::cap_query_sql(&s.sql, cap)
                                    } else {
                                        s.sql.clone()
                                    };
                                    (sql, d1_typed_binds(&s.parameters))
                                })
                                .collect();
                            self.run_batch_with_timeout(&fin_stmts, timeout).await?;
                        }
                    }
                    return Ok(reply);
                }
                Err(err) if is_ambiguous_d1(&err) && attempt + 1 < ATOMIC_HTTP_ATTEMPTS => {
                    sleep_before_d1_retry_bounded(attempt, None, deadline).await?;
                    last_err = Some(err);
                }
                Err(err) => return Err(err),
            }
        }
        Err(last_err.unwrap_or_else(|| ambiguous_d1("exhausted retries")))
    }
}

/// HTTP timeout for one D1 batch, capped by the guest-visible deadline.
fn d1_http_timeout(deadline_unix_ms: Option<u64>) -> std::result::Result<Duration, DbErr> {
    match deadline_unix_ms {
        Some(dl) => {
            let now = d1_unix_now_ms();
            if now >= dl {
                return Err(DbErr::Custom(
                    "deadline_exceeded: atomic deadline elapsed".into(),
                ));
            }
            Ok(Duration::from_millis(dl - now).min(super::d1::D1_REQUEST_TIMEOUT))
        }
        None => Ok(super::d1::D1_REQUEST_TIMEOUT),
    }
}

/// Checks cancel/deadline inject plus a guest-visible `deadlineUnixMs`.
fn check_d1_session(
    phase: bookclerk_db_exec::AtomicInterruptPhase,
    deadline_unix_ms: Option<u64>,
) -> std::result::Result<(), DbErr> {
    use bookclerk_db_exec::{AtomicInterruptKind, AtomicInterruptPhase};
    let kind = bookclerk_db_exec::consume_atomic_interrupt(phase);
    let expired = deadline_unix_ms.is_some_and(|ms| d1_unix_now_ms() >= ms);
    let kind = kind.or_else(|| expired.then_some(AtomicInterruptKind::Deadline));
    let Some(kind) = kind else {
        return Ok(());
    };
    match phase {
        AtomicInterruptPhase::AroundCommit => Err(ambiguous_d1("session interrupt at HTTP return")),
        AtomicInterruptPhase::BeforeBegin | AtomicInterruptPhase::BetweenStatements => {
            let msg = match kind {
                AtomicInterruptKind::Cancel => "cancelled: atomic session cancelled",
                AtomicInterruptKind::Deadline => "deadline_exceeded: atomic deadline elapsed",
            };
            Err(DbErr::Custom(msg.into()))
        }
    }
}

/// Current unix time in milliseconds.
fn d1_unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// D1 HTTP params: typed nulls become JSON null; text is never `b64:`-decoded.
fn d1_typed_binds(params: &[DbValue]) -> Vec<JsonValue> {
    params
        .iter()
        .map(|v| match v {
            DbValue::Null(_) => JsonValue::Null,
            DbValue::Boolean(b) => JsonValue::Bool(*b),
            DbValue::Int64(n) => JsonValue::from(*n),
            DbValue::Float64(n) => JsonValue::from(*n),
            DbValue::Text(s) => JsonValue::String(s.clone()),
            DbValue::Bytes(b) => JsonValue::String(format!(
                "b64:{}",
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b)
            )),
        })
        .collect()
}

/// D1 HTTP params are untyped JSON; typed `$sea_null` objects become SQL NULL.
fn d1_wire_binds(binds: &[JsonValue]) -> Vec<JsonValue> {
    binds
        .iter()
        .map(|v| {
            if sea_null_kind(v).is_some() {
                JsonValue::Null
            } else {
                v.clone()
            }
        })
        .collect()
}

/// Waits before a D1 retry, honoring `Retry-After` or a capped exponential backoff.
async fn sleep_before_d1_retry_bounded(
    attempt: usize,
    retry_after: Option<Duration>,
    deadline_unix_ms: Option<u64>,
) -> std::result::Result<(), DbErr> {
    let delay = retry_after.unwrap_or_else(|| {
        Duration::from_millis((50u64.saturating_mul(3u64.saturating_pow(attempt as u32))).min(400))
    });
    let delay = delay.min(Duration::from_secs(5));
    if let Some(dl) = deadline_unix_ms {
        let now = d1_unix_now_ms();
        if now >= dl {
            return Err(DbErr::Custom(
                "deadline_exceeded: atomic deadline elapsed".into(),
            ));
        }
        let remain = Duration::from_millis(dl - now);
        tokio::time::sleep(delay.min(remain)).await;
        if d1_unix_now_ms() >= dl {
            return Err(DbErr::Custom(
                "deadline_exceeded: atomic deadline elapsed".into(),
            ));
        }
        return Ok(());
    }
    tokio::time::sleep(delay).await;
    Ok(())
}

/// True when a D1 HTTP/parse failure may have already committed the batch.
pub fn is_ambiguous_d1(err: &DbErr) -> bool {
    err.to_string().contains("D1 ambiguous")
}

/// Maps a D1 [`DbErr`] onto the guest ABI: retryable/ambiguous → `unavailable`,
/// client 4xx → `invalid_params`, engine codes via the shared mapper.
#[must_use]
pub fn plugin_error_from_d1(err: DbErr) -> PluginError {
    if bookclerk_db_exec::is_guest_receipt_result_lost(&err) {
        return PluginError::unavailable(err.to_string());
    }
    if is_ambiguous_d1(&err) {
        return PluginError::unavailable(err.to_string());
    }
    if let Some(status) = permanent_http_status(&err) {
        if (400..500).contains(&status) {
            return PluginError::invalid_params(err.to_string());
        }
    }
    bookclerk_plugin_sdk::database_adapter::plugin_error_from_engine(err)
}

/// Extracts a permanent `D1 HTTP {status}` code from a [`DbErr`], if present.
fn permanent_http_status(err: &DbErr) -> Option<u16> {
    let text = err.to_string();
    let idx = text.find("D1 HTTP ")?;
    text[idx + "D1 HTTP ".len()..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// Builds a [`DbErr`] whose message marks the batch as possibly already committed.
fn ambiguous_d1(msg: impl std::fmt::Display) -> DbErr {
    DbErr::Custom(format!("D1 ambiguous response: {msg}"))
}

/// Fails closed on DML `RETURNING` that D1 HTTP cannot prove is at most one row.
///
/// Cloudflare commits the batch before JSON is parsed. `Returning` requires a
/// host-IR `maxRows = 1`. Top-level `;` (multi-statement SQL) and multi-tuple
/// `VALUES` are rejected before HTTP.
fn reject_unbounded_returning(plan: &DbAtomicPlan) -> std::result::Result<(), DbErr> {
    let cap = DbConnectResult::d1().max_result_rows;
    for (i, stmt) in plan.statements.iter().enumerate() {
        if sql_has_top_level_semicolon(&stmt.sql) {
            return Err(DbErr::Custom(format!(
                "D1 statement {i} contains multiple SQL statements; maxResultRows is {cap}"
            )));
        }
        let looks_returning = matches!(stmt.kind, DbPlanStatementKind::Returning);
        if !looks_returning {
            continue;
        }
        if stmt.max_rows != 1 {
            return Err(DbErr::Custom(format!(
                "D1 statement {i} Returning is not proven bounded; maxResultRows is {cap}"
            )));
        }
        if has_top_level_keyword(&stmt.sql, "VALUES") {
            let tuples = count_top_level_values_tuples(&stmt.sql);
            if tuples != 1 {
                return Err(DbErr::Custom(format!(
                    "D1 statement {i} Returning VALUES is not a single tuple ({tuples}); maxResultRows is {cap}"
                )));
            }
        }
    }
    Ok(())
}

/// Fails closed on DML `RETURNING` that D1 HTTP cannot prove is at most one row.
///
/// # Errors
///
/// Returns when SQL is multi-statement, `max_rows != 1`, or `VALUES` is not a
/// single tuple.
fn reject_unbounded_returning_typed(
    statements: &[TypedDbStatement],
) -> std::result::Result<(), DbErr> {
    let cap = DbConnectResult::d1().max_result_rows;
    for (i, stmt) in statements.iter().enumerate() {
        if sql_has_top_level_semicolon(&stmt.sql) {
            return Err(DbErr::Custom(format!(
                "D1 statement {i} contains multiple SQL statements; maxResultRows is {cap}"
            )));
        }
        let looks_returning = matches!(stmt.kind, DbPlanStatementKind::Returning);
        if !looks_returning {
            continue;
        }
        if stmt.max_rows != 1 {
            return Err(DbErr::Custom(format!(
                "D1 statement {i} Returning is not proven bounded; maxResultRows is {cap}"
            )));
        }
        if has_top_level_keyword(&stmt.sql, "VALUES") {
            let tuples = count_top_level_values_tuples(&stmt.sql);
            if tuples != 1 {
                return Err(DbErr::Custom(format!(
                    "D1 statement {i} Returning VALUES is not a single tuple ({tuples}); maxResultRows is {cap}"
                )));
            }
        }
    }
    Ok(())
}

/// Maps one D1 HTTP JSON cell onto a typed [`DbValue`] (strings stay text).
///
/// # Errors
///
/// Returns when the cell is an array, object, or a non-finite number.
fn d1_json_cell_to_db_value(v: &JsonValue) -> Result<DbValue, String> {
    match v {
        JsonValue::Null => Ok(DbValue::Null(DbType::Unspecified)),
        JsonValue::Bool(b) => Ok(DbValue::Boolean(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                return Ok(DbValue::Int64(i));
            }
            if let Some(u) = n.as_u64() {
                let i = i64::try_from(u)
                    .map_err(|_| format!("unsigned integer {u} overflows int64"))?;
                return Ok(DbValue::Int64(i));
            }
            let f = n
                .as_f64()
                .ok_or_else(|| "number is not a finite float64".to_string())?;
            if !f.is_finite() {
                return Err("float64 value is not finite".into());
            }
            Ok(DbValue::Float64(f))
        }
        JsonValue::String(s) => Ok(DbValue::Text(s.clone())),
        JsonValue::Array(_) => Err("arrays are not a baseline DbValue".into()),
        JsonValue::Object(_) => Err("objects are not a baseline DbValue".into()),
    }
}

/// Column metadata for a D1 HTTP result page.
///
/// Names come from the first row object when present. Empty pages (and `{}`
/// all-null objects that omit keys) fall back to the SELECT-list identifiers.
fn d1_result_columns(sql: &str, raw_rows: &[JsonValue]) -> Vec<DbColumn> {
    if let Some(map) = raw_rows.first().and_then(JsonValue::as_object) {
        if !map.is_empty() {
            return map
                .keys()
                .map(|name| DbColumn {
                    name: name.clone(),
                    db_type: DbType::Unspecified,
                })
                .collect();
        }
    }
    select_list_column_names(sql)
        .into_iter()
        .map(|name| DbColumn {
            name,
            db_type: DbType::Unspecified,
        })
        .collect()
}

/// Fills [`DbType`] from the first non-null cell in each column.
fn refine_column_types(columns: &mut [DbColumn], rows: &[DbRow]) {
    for (i, col) in columns.iter_mut().enumerate() {
        if col.db_type != DbType::Unspecified {
            continue;
        }
        for row in rows {
            match row.values.get(i) {
                Some(DbValue::Null(_)) | None => continue,
                Some(DbValue::Boolean(_)) => col.db_type = DbType::Bool,
                Some(DbValue::Int64(_)) => col.db_type = DbType::Int64,
                Some(DbValue::Float64(_)) => col.db_type = DbType::Float64,
                Some(DbValue::Text(_)) => col.db_type = DbType::Text,
                Some(DbValue::Bytes(_)) => col.db_type = DbType::Bytes,
            }
            break;
        }
    }
}

/// SELECT-list identifiers used when D1 HTTP returns no row objects.
fn select_list_column_names(sql: &str) -> Vec<String> {
    let select = match select_list_slice(sql) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut names = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let bytes = select.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                if let Some(name) = select_item_name(&select[start..i]) {
                    names.push(name);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if let Some(name) = select_item_name(&select[start..]) {
        names.push(name);
    }
    names
}

/// Text between the main `SELECT` and `FROM` (depth-0).
fn select_list_slice(sql: &str) -> Option<&str> {
    let upper = sql.to_ascii_uppercase();
    let mut depth = 0i32;
    let bytes = upper.as_bytes();
    let mut i = 0usize;
    let mut select_at = None;
    while i + 6 <= bytes.len() {
        match bytes[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'S' if depth == 0
                && bytes[i..].starts_with(b"SELECT")
                && !ident_cont_at(bytes, i + 6) =>
            {
                select_at = Some(i + 6);
                i += 6;
            }
            b'F' if depth == 0
                && select_at.is_some()
                && bytes[i..].starts_with(b"FROM")
                && !ident_cont_at(bytes, i + 4) =>
            {
                return Some(sql[select_at?..i].trim());
            }
            _ => i += 1,
        }
    }
    None
}

/// True when `bytes[i]` continues an identifier (`[A-Za-z0-9_]`).
fn ident_cont_at(bytes: &[u8], i: usize) -> bool {
    bytes
        .get(i)
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

/// Last identifier in a SELECT item (`col`, `table.col`, `expr AS alias`).
fn select_item_name(item: &str) -> Option<String> {
    let item = item.trim();
    if item.is_empty() || item == "*" || item.ends_with(".*") {
        return None;
    }
    let upper = item.to_ascii_uppercase();
    let token = if let Some(idx) = upper.rfind(" AS ") {
        item[idx + 4..].trim()
    } else {
        item.rsplit([' ', '.']).next().unwrap_or(item).trim()
    };
    let token = token.trim_matches(|c| c == '"' || c == '`' || c == '\'');
    if token.is_empty()
        || token == "*"
        || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(token.to_string())
}

/// Parses a D1 HTTP batch body into [`ExecuteReply`] and encodes before return.
///
/// # Errors
///
/// Returns [`DbErr`] when the body is malformed, a row fails conversion, or
/// the encoded reply exceeds `maxAtomicResultBytes` (ambiguous after HTTP).
fn parse_typed_batch(
    req: &ExecuteRequest,
    value: &JsonValue,
    started: std::time::Instant,
) -> std::result::Result<ExecuteReply, DbErr> {
    let Some(arr) = value.get("result").and_then(JsonValue::as_array) else {
        return Err(ambiguous_d1("batch response missing result array"));
    };
    if arr.len() != req.statements.len() {
        return Err(ambiguous_d1(format!(
            "expected {} statement results, got {}",
            req.statements.len(),
            arr.len()
        )));
    }
    let caps = DbConnectResult::d1();
    let row_cap = usize::try_from(caps.max_result_rows).unwrap_or(1_000);
    let mut statements = Vec::with_capacity(arr.len());
    for (i, entry) in arr.iter().enumerate() {
        if entry.get("success").and_then(JsonValue::as_bool) == Some(false) {
            return Err(DbErr::Custom(format!(
                "D1 batch statement {i} failed: {entry}"
            )));
        }
        let kind = req.statements[i].kind;
        let selection = req.statements[i].result_selection;
        let changes = entry
            .get("meta")
            .and_then(|m| m.get("changes"))
            .and_then(JsonValue::as_u64)
            .unwrap_or(0);
        let stmt_result = match selection {
            DbResultSelection::AffectedRows | DbResultSelection::Discard => {
                let n = if matches!(selection, DbResultSelection::Discard)
                    || matches!(kind, DbPlanStatementKind::Select)
                {
                    0
                } else {
                    changes
                };
                StatementResult::from_affected(n)
            }
            DbResultSelection::Rows => {
                let raw_rows = entry
                    .get("results")
                    .and_then(JsonValue::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if raw_rows.len() > row_cap {
                    return Err(ambiguous_d1(format!(
                        "D1 statement {i} returned {} rows; maxResultRows is {row_cap}",
                        raw_rows.len()
                    )));
                }
                let mut columns: Vec<DbColumn> =
                    d1_result_columns(&req.statements[i].sql, raw_rows);
                let mut rows = Vec::new();
                for row in raw_rows {
                    let Some(map) = row.as_object() else {
                        return Err(ambiguous_d1(format!(
                            "D1 statement {i} row is not an object"
                        )));
                    };
                    let mut values = Vec::with_capacity(columns.len());
                    for col in &columns {
                        let cell = map.get(&col.name).unwrap_or(&JsonValue::Null);
                        values.push(d1_json_cell_to_db_value(cell).map_err(DbErr::Custom)?);
                    }
                    rows.push(DbRow { values });
                }
                refine_column_types(&mut columns, &rows);
                let mut result = StatementResult::from_rows(columns, rows).map_err(ambiguous_d1)?;
                result.rows_affected = match kind {
                    DbPlanStatementKind::Select => 0,
                    DbPlanStatementKind::Returning => {
                        u64::try_from(result.rows.len()).unwrap_or(u64::MAX)
                    }
                    DbPlanStatementKind::Execute => changes,
                };
                result
            }
        };
        if caps.max_result_bytes > 0 {
            let used = encoded_statement_result_bytes(&stmt_result)
                .map(|b| b.len())
                .unwrap_or(usize::MAX);
            let cap = usize::try_from(caps.max_result_bytes).unwrap_or(usize::MAX);
            if used > cap {
                return Err(ambiguous_d1(format!(
                    "query result is {used} bytes; maxResultBytes is {}",
                    caps.max_result_bytes
                )));
            }
        }
        statements.push(stmt_result);
    }
    let db_execution_us = d1_sql_duration_us(value);
    let reply = ExecuteReply {
        operation_id: req.operation_id.clone(),
        statements,
        timing: DbTiming {
            attempt_elapsed_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            db_execution_us: db_execution_us.unwrap_or(0),
            db_timing_source: db_execution_us
                .map(|_| "d1_sql_duration".to_string())
                .unwrap_or_default(),
        },
    };
    reply.validate_positional().map_err(ambiguous_d1)?;
    match encoded_execute_reply_bytes(&reply) {
        Ok(bytes) => {
            let cap = usize::try_from(caps.max_atomic_result_bytes).unwrap_or(0);
            if cap > 0 && bytes.len() > cap {
                return Err(ambiguous_d1(format!(
                    "atomic result is {} bytes; maxAtomicResultBytes is {cap}",
                    bytes.len()
                )));
            }
        }
        Err(err) => {
            return Err(ambiguous_d1(format!(
                "failed to encode ExecuteReply after D1 HTTP commit: {err}"
            )));
        }
    }
    Ok(reply)
}

/// True when a top-level semicolon would start another statement.
fn sql_has_top_level_semicolon(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut in_squote = false;
    let mut in_dquote = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_squote {
            if c == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_squote = false;
            }
            i += 1;
            continue;
        }
        if in_dquote {
            if c == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    i += 2;
                    continue;
                }
                in_dquote = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' => in_squote = true,
            b'"' => in_dquote = true,
            b'(' => depth = depth.saturating_add(1),
            b')' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => {
                let rest = sql[i + 1..].trim();
                if !rest.is_empty() {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Number of top-level `VALUES` tuples (`(1),(2)` → 2). `0` when none parsed.
fn count_top_level_values_tuples(sql: &str) -> usize {
    let Some(idx) = top_level_keyword_index(sql, "VALUES") else {
        return 0;
    };
    let bytes = sql.as_bytes();
    let mut i = idx + "VALUES".len();
    let mut depth = 0usize;
    let mut tuples = 0usize;
    let mut in_squote = false;
    let mut in_dquote = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_squote {
            if c == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_squote = false;
            }
            i += 1;
            continue;
        }
        if in_dquote {
            if c == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    i += 2;
                    continue;
                }
                in_dquote = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' => in_squote = true,
            b'"' => in_dquote = true,
            b'(' => {
                if depth == 0 {
                    tuples = tuples.saturating_add(1);
                }
                depth = depth.saturating_add(1);
            }
            b')' => depth = depth.saturating_sub(1),
            _ => {
                if depth == 0 {
                    let rest = &sql[i..];
                    if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case("RETURNING") {
                        break;
                    }
                    if c == b';' {
                        break;
                    }
                }
            }
        }
        i += 1;
    }
    tuples
}

/// Byte offset of a top-level keyword, if present.
fn top_level_keyword_index(sql: &str, keyword: &str) -> Option<usize> {
    let mut found = None;
    for_each_top_level_keyword(sql, |idx, kw| {
        if kw.eq_ignore_ascii_case(keyword) {
            found = Some(idx);
        }
    });
    found
}

/// True when `keyword` appears at parenthesis depth 0 (not inside quotes).
fn has_top_level_keyword(sql: &str, keyword: &str) -> bool {
    let mut found = false;
    for_each_top_level_keyword(sql, |_, kw| {
        if kw.eq_ignore_ascii_case(keyword) {
            found = true;
        }
    });
    found
}

/// Invokes `on_keyword` for each unquoted, top-level identifier.
fn for_each_top_level_keyword(sql: &str, mut on_keyword: impl FnMut(usize, &str)) {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut in_squote = false;
    let mut in_dquote = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_squote {
            if c == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_squote = false;
            }
            i += 1;
            continue;
        }
        if in_dquote {
            if c == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    i += 2;
                    continue;
                }
                in_dquote = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' => {
                in_squote = true;
                i += 1;
            }
            b'"' => {
                in_dquote = true;
                i += 1;
            }
            b'(' => {
                depth = depth.saturating_add(1);
                i += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            _ if depth == 0 && c.is_ascii_alphabetic() => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                on_keyword(start, &sql[start..i]);
            }
            _ => i += 1,
        }
    }
}

/// Sums D1 `sql_duration_ms` timings from a batch response and returns microseconds.
fn d1_sql_duration_us(raw: &JsonValue) -> Option<u64> {
    let arr = raw.get("result")?.as_array()?;
    let mut ms = 0.0_f64;
    let mut any = false;
    for entry in arr {
        if let Some(duration) = entry
            .get("meta")
            .and_then(|m| m.get("timings"))
            .and_then(|t| t.get("sql_duration_ms"))
            .and_then(JsonValue::as_f64)
        {
            ms += duration;
            any = true;
        }
    }
    any.then_some((ms * 1000.0) as u64)
}

/// Parses a D1 batch for a host-authored [`DbAtomicPlan`].
fn parse_generic_batch(
    plan: &DbAtomicPlan,
    value: &JsonValue,
    operation_id: String,
    started: std::time::Instant,
) -> std::result::Result<DbPlanExecResult, DbErr> {
    let statements = parse_batch_results(plan, value)?;
    if statements.len() != plan.statements.len() {
        return Err(ambiguous_d1(format!(
            "expected {} statement results, got {}",
            plan.statements.len(),
            statements.len()
        )));
    }
    let cap = usize::try_from(DbConnectResult::d1().max_result_rows).unwrap_or(1_000);
    for (i, stmt) in statements.iter().enumerate() {
        if stmt.rows.len() > cap {
            return Err(ambiguous_d1(format!(
                "D1 statement {i} returned {} rows; maxResultRows is {cap}",
                stmt.rows.len()
            )));
        }
    }
    let db_execution_us = d1_sql_duration_us(value);
    let result = DbPlanExecResult {
        operation_id,
        statements,
        timing: Some(DbAtomicTiming {
            attempt_elapsed_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            db_execution_us,
            db_timing_source: db_execution_us.map(|_| "d1_sql_duration".into()),
        }),
    };
    let atomic_cap = usize::try_from(DbConnectResult::d1().max_atomic_result_bytes).unwrap_or(0);
    if atomic_cap > 0 {
        let used = serde_json::to_vec(&result)
            .map(|b| b.len())
            .unwrap_or(usize::MAX);
        if used > atomic_cap {
            return Err(ambiguous_d1(format!(
                "atomic result is {used} bytes; maxAtomicResultBytes is {atomic_cap}"
            )));
        }
    }
    Ok(result)
}

/// Parses the D1 `result` array; a `success: false` entry is a hard statement failure.
fn parse_batch_results(
    plan: &DbAtomicPlan,
    value: &JsonValue,
) -> std::result::Result<Vec<DbPlanStmtExecResult>, DbErr> {
    let Some(arr) = value.get("result").and_then(JsonValue::as_array) else {
        return Err(ambiguous_d1("batch response missing result array"));
    };
    let caps = DbConnectResult::d1();
    let row_cap = usize::try_from(caps.max_result_rows).unwrap_or(1_000);
    let result_cap = usize::try_from(caps.max_result_bytes).unwrap_or(usize::MAX);
    let cell_cap = usize::try_from(caps.max_cell_bytes).unwrap_or(usize::MAX);
    let mut out = Vec::with_capacity(arr.len());
    for (i, entry) in arr.iter().enumerate() {
        if entry.get("success").and_then(JsonValue::as_bool) == Some(false) {
            return Err(DbErr::Custom(format!(
                "D1 batch statement {i} failed: {entry}"
            )));
        }
        let raw_rows = entry
            .get("results")
            .and_then(JsonValue::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut rows = Vec::new();
        let mut used = 0usize;
        for row in raw_rows {
            if caps.max_cell_bytes > 0 {
                if let JsonValue::Object(map) = row {
                    for (name, cell) in map {
                        let n = bookclerk_db_exec::json_cell_utf8_len(cell);
                        if n > cell_cap {
                            return Err(ambiguous_d1(format!(
                                "D1 statement {i} column `{name}` is {n} bytes; maxCellBytes is {}",
                                caps.max_cell_bytes
                            )));
                        }
                    }
                }
            }
            if caps.max_result_bytes > 0 {
                used = used.saturating_add(row.to_string().len());
                if used > result_cap {
                    return Err(ambiguous_d1(format!(
                        "D1 statement {i} result is {used} bytes; maxResultBytes is {}",
                        caps.max_result_bytes
                    )));
                }
            }
            rows.push(row.clone());
            if rows.len() > row_cap {
                break;
            }
        }
        let changes = entry
            .get("meta")
            .and_then(|m| m.get("changes"))
            .and_then(JsonValue::as_u64)
            .unwrap_or(0);
        let kind = plan
            .statements
            .get(i)
            .map(|s| s.kind)
            .unwrap_or(DbPlanStatementKind::Returning);
        let rows_affected = match kind {
            DbPlanStatementKind::Select => 0,
            DbPlanStatementKind::Returning => u64::try_from(rows.len()).unwrap_or(u64::MAX),
            DbPlanStatementKind::Execute => changes,
        };
        out.push(DbPlanStmtExecResult {
            rows,
            rows_affected,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_result_array_is_ambiguous() {
        let plan = DbAtomicPlan {
            statements: vec![],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let started = std::time::Instant::now();
        let err = parse_generic_batch(&plan, &json!({}), "op-1".into(), started).unwrap_err();
        assert!(is_ambiguous_d1(&err), "{err}");
    }

    #[test]
    fn statement_failure_is_not_ambiguous() {
        let plan = DbAtomicPlan {
            statements: vec![bookclerk_db_exec::DbPlanStatement {
                sql: "INSERT INTO t (k) VALUES ('a')".into(),
                binds: vec![],
                kind: DbPlanStatementKind::Execute,
                max_rows: 0,
            }],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let value = json!({
            "result": [{ "success": false, "error": "constraint" }]
        });
        let err = parse_batch_results(&plan, &value).unwrap_err();
        assert!(!is_ambiguous_d1(&err), "{err}");
    }

    #[test]
    fn parse_caps_result_rows() {
        let plan = DbAtomicPlan {
            statements: vec![bookclerk_db_exec::DbPlanStatement {
                sql: "SELECT 1".into(),
                binds: vec![],
                kind: bookclerk_plugin_sdk::DbPlanStatementKind::Select,
                max_rows: 0,
            }],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let mut rows = Vec::new();
        for i in 0..1_050 {
            rows.push(json!({ "n": i }));
        }
        let value = json!({
            "result": [{ "success": true, "results": rows, "meta": { "changes": 0 } }]
        });
        let started = std::time::Instant::now();
        let err = parse_generic_batch(&plan, &value, "op-cap".into(), started).unwrap_err();
        assert!(
            err.to_string().contains("maxResultRows"),
            "D1 must fail closed on over-cap rows: {err}"
        );
        assert!(
            is_ambiguous_d1(&err),
            "overflow after HTTP commit must be ambiguous: {err}"
        );
        let at_cap = json!({
            "result": [{ "success": true, "results": rows[..1000].to_vec(), "meta": { "changes": 0 } }]
        });
        let exec = parse_generic_batch(&plan, &at_cap, "op-cap-ok".into(), started).unwrap();
        assert_eq!(exec.statements[0].rows.len(), 1_000);
    }

    fn stmt(sql: &str, kind: DbPlanStatementKind) -> bookclerk_db_exec::DbPlanStatement {
        bookclerk_db_exec::DbPlanStatement::new(sql, vec![], kind)
    }

    fn plan_of(sql: &str, kind: DbPlanStatementKind) -> DbAtomicPlan {
        DbAtomicPlan {
            statements: vec![stmt(sql, kind)],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        }
    }

    #[test]
    fn host_insert_returning_is_proven_single_row() {
        let sql = "INSERT OR IGNORE INTO event_deliveries (id) \
             SELECT ? WHERE EXISTS (SELECT 1 FROM domain_events WHERE id = ?) RETURNING id";
        let mut plan = plan_of(sql, DbPlanStatementKind::Returning);
        plan.statements[0].max_rows = 1;
        reject_unbounded_returning(&plan).unwrap();
    }

    #[test]
    fn recursive_insert_returning_is_rejected_before_http() {
        let sql = "WITH RECURSIVE t(id) AS (SELECT 0 UNION ALL SELECT id+1 FROM t WHERE id < 5) \
             INSERT INTO vec_ret_ins (id) SELECT id FROM t RETURNING id";
        let err =
            reject_unbounded_returning(&plan_of(sql, DbPlanStatementKind::Returning)).unwrap_err();
        assert!(err.to_string().contains("maxResultRows"), "{err}");
        assert!(!is_ambiguous_d1(&err), "{err}");
    }

    #[test]
    fn multi_values_returning_is_not_proven() {
        let sql = "INSERT INTO t(id) VALUES (1),(2) RETURNING id";
        let mut plan = plan_of(sql, DbPlanStatementKind::Returning);
        plan.statements[0].max_rows = 1;
        let err = reject_unbounded_returning(&plan).unwrap_err();
        assert!(err.to_string().contains("VALUES"), "{err}");
        assert!(err.to_string().contains("maxResultRows"), "{err}");
        assert_eq!(count_top_level_values_tuples(sql), 2);
    }

    #[test]
    fn semicolon_joined_sql_is_rejected() {
        let sql = "INSERT INTO t(id) VALUES (1); INSERT INTO t(id) VALUES (2)";
        let err =
            reject_unbounded_returning(&plan_of(sql, DbPlanStatementKind::Execute)).unwrap_err();
        assert!(err.to_string().contains("multiple SQL statements"), "{err}");
    }

    fn typed_select(sql: &str) -> ExecuteRequest {
        ExecuteRequest {
            operation_id: "d1".into(),
            request_hash: String::new(),
            statements: vec![TypedDbStatement {
                sql: sql.into(),
                parameters: vec![],
                kind: DbPlanStatementKind::Select,
                max_rows: 8,
                result_selection: DbResultSelection::Rows,
            }],
            deadline_unix_ms: 0,
        }
    }

    #[test]
    fn empty_select_uses_select_list_column_names() {
        let req = typed_select("SELECT id, title FROM books WHERE 0");
        let value = json!({
            "result": [{ "success": true, "results": [], "meta": { "changes": 0 } }]
        });
        let reply = parse_typed_batch(&req, &value, std::time::Instant::now()).unwrap();
        let names: Vec<&str> = reply.statements[0]
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, vec!["id", "title"]);
        assert!(reply.statements[0].rows.is_empty());
    }

    #[test]
    fn all_null_row_keeps_column_names_and_null_cells() {
        let req = typed_select("SELECT id, title FROM books");
        let value = json!({
            "result": [{
                "success": true,
                "results": [{ "id": null, "title": null }],
                "meta": { "changes": 0 }
            }]
        });
        let reply = parse_typed_batch(&req, &value, std::time::Instant::now()).unwrap();
        assert_eq!(reply.statements[0].columns.len(), 2);
        assert_eq!(
            reply.statements[0].rows[0].values,
            vec![
                DbValue::Null(DbType::Unspecified),
                DbValue::Null(DbType::Unspecified)
            ]
        );
    }

    #[test]
    fn text_starting_with_b64_stays_text() {
        let req = typed_select("SELECT note FROM books");
        let value = json!({
            "result": [{
                "success": true,
                "results": [{ "note": "b64:not-bytes" }],
                "meta": { "changes": 0 }
            }]
        });
        let reply = parse_typed_batch(&req, &value, std::time::Instant::now()).unwrap();
        assert_eq!(
            reply.statements[0].rows[0].values[0],
            DbValue::Text("b64:not-bytes".into())
        );
        assert_eq!(reply.statements[0].columns[0].db_type, DbType::Text);
    }
}
