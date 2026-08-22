//! Generic D1 HTTP `batch()` executor for host-authored SQL plans.
//!
//! The guest does not parse Bookclerk operation names. The host compiles
//! domain work into [`DbAtomicPlan`]; this module runs that list as one
//! D1 `{ "batch": [...] }` SQL transaction and returns statement results.

use std::time::Duration;

use bookclerk_plugin_sdk::{
    sea_null_kind, DbAtomicPlan, DbAtomicRequest, DbAtomicTiming, DbConnectResult,
    DbPlanExecResult, DbPlanStatementKind, DbPlanStmtExecResult,
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
pub fn plugin_error_from_d1(err: DbErr) -> bookclerk_plugin_sdk::PluginError {
    if is_ambiguous_d1(&err) {
        return bookclerk_plugin_sdk::PluginError::unavailable(err.to_string());
    }
    if let Some(status) = permanent_http_status(&err) {
        if (400..500).contains(&status) {
            return bookclerk_plugin_sdk::PluginError::invalid_params(err.to_string());
        }
    }
    bookclerk_db_guest::plugin_error_from_engine(err)
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
/// Cloudflare commits the batch before JSON is parsed. Multi-row `RETURNING`
/// (recursive CTEs, `FROM` relations, `UNION`, `UPDATE`/`DELETE`) is rejected
/// before HTTP. Host 1-row `INSERT … SELECT ?, ? WHERE … RETURNING` is allowed.
fn reject_unbounded_returning(plan: &DbAtomicPlan) -> std::result::Result<(), DbErr> {
    let cap = DbConnectResult::d1().max_result_rows;
    for (i, stmt) in plan.statements.iter().enumerate() {
        let looks_returning = matches!(stmt.kind, DbPlanStatementKind::Returning)
            || (matches!(stmt.kind, DbPlanStatementKind::Query)
                && has_top_level_keyword(&stmt.sql, "RETURNING"));
        if !looks_returning {
            continue;
        }
        if returning_is_provably_single_row(&stmt.sql) {
            continue;
        }
        return Err(DbErr::Custom(format!(
            "D1 statement {i} Returning is not proven bounded; maxResultRows is {cap}"
        )));
    }
    Ok(())
}

/// True for `INSERT … SELECT <scalars> WHERE … RETURNING` / `INSERT … VALUES … RETURNING`.
fn returning_is_provably_single_row(sql: &str) -> bool {
    let mut saw_insert = false;
    let mut saw_update_or_delete = false;
    let mut banned = false;
    for_each_top_level_keyword(sql, |_, kw| {
        let upper = kw.to_ascii_uppercase();
        match upper.as_str() {
            "INSERT" => saw_insert = true,
            "UPDATE" | "DELETE" => saw_update_or_delete = true,
            "FROM" | "UNION" | "RECURSIVE" | "GENERATE_SERIES" => banned = true,
            _ => {}
        }
    });
    saw_insert && !saw_update_or_delete && !banned && has_top_level_keyword(sql, "RETURNING")
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
    Ok(DbPlanExecResult {
        operation_id,
        statements,
        timing: Some(DbAtomicTiming {
            attempt_elapsed_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            db_execution_us,
            db_timing_source: db_execution_us.map(|_| "d1_sql_duration".into()),
        }),
    })
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
            .unwrap_or(DbPlanStatementKind::Query);
        let rows_affected = match kind {
            DbPlanStatementKind::Select => 0,
            DbPlanStatementKind::Returning | DbPlanStatementKind::Query => {
                u64::try_from(rows.len()).unwrap_or(u64::MAX)
            }
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
            statements: vec![bookclerk_plugin_sdk::DbPlanStatement {
                sql: "INSERT INTO t (k) VALUES ('a')".into(),
                binds: vec![],
                kind: DbPlanStatementKind::Execute,
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
            statements: vec![bookclerk_plugin_sdk::DbPlanStatement {
                sql: "SELECT 1".into(),
                binds: vec![],
                kind: bookclerk_plugin_sdk::DbPlanStatementKind::Query,
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

    fn stmt(sql: &str, kind: DbPlanStatementKind) -> bookclerk_plugin_sdk::DbPlanStatement {
        bookclerk_plugin_sdk::DbPlanStatement {
            sql: sql.into(),
            binds: vec![],
            kind,
        }
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
        reject_unbounded_returning(&plan_of(sql, DbPlanStatementKind::Returning)).unwrap();
        assert!(returning_is_provably_single_row(sql));
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
}
