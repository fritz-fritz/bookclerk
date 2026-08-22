//! Generic D1 HTTP `batch()` executor for host-authored SQL plans.
//!
//! The guest does not parse Bookclerk operation names. The host compiles
//! domain work into [`DbAtomicPlan`]; this module runs that list as one
//! D1 `{ "batch": [...] }` SQL transaction and returns statement results.

use std::time::Duration;

use bookclerk_plugin_sdk::{
    sea_null_kind, DbAtomicPlan, DbAtomicRequest, DbAtomicTiming, DbConnectResult,
    DbPlanExecResult, DbPlanStmtExecResult,
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
            bookclerk_library::AtomicInterruptPhase::BeforeBegin,
            req.deadline_unix_ms,
        )?;
        let plan = req
            .plan
            .clone()
            .ok_or_else(|| DbErr::Custom("dbAtomic requires a host-authored executePlan".into()))?;
        let statements: Vec<SqlStmt> = plan
            .statements
            .iter()
            .map(|s| (s.sql.clone(), d1_wire_binds(&s.binds)))
            .collect();
        let mut last_err = None;
        for attempt in 0..ATOMIC_HTTP_ATTEMPTS {
            let raw = match self.run_batch(&statements).await {
                Ok(value) => {
                    check_d1_session(
                        bookclerk_library::AtomicInterruptPhase::AroundCommit,
                        req.deadline_unix_ms,
                    )?;
                    value
                }
                Err(err) if err.is_retryable() && attempt + 1 < ATOMIC_HTTP_ATTEMPTS => {
                    sleep_before_d1_retry(attempt, err.retry_after()).await;
                    last_err = Some(DbErr::from(err));
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
            match parse_generic_batch(&plan, &raw, req.operation_id.clone(), started) {
                Ok(result) => return Ok(result),
                Err(err) if is_ambiguous_d1(&err) && attempt + 1 < ATOMIC_HTTP_ATTEMPTS => {
                    sleep_before_d1_retry(attempt, None).await;
                    last_err = Some(err);
                }
                Err(err) => return Err(err),
            }
        }
        Err(last_err.unwrap_or_else(|| ambiguous_d1("exhausted retries")))
    }
}

/// Checks cancel/deadline inject plus a guest-visible `deadlineUnixMs`.
fn check_d1_session(
    phase: bookclerk_library::AtomicInterruptPhase,
    deadline_unix_ms: Option<u64>,
) -> std::result::Result<(), DbErr> {
    use bookclerk_library::{AtomicInterruptKind, AtomicInterruptPhase};
    let kind = bookclerk_library::consume_atomic_interrupt(phase);
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
async fn sleep_before_d1_retry(attempt: usize, retry_after: Option<Duration>) {
    let delay = retry_after.unwrap_or_else(|| {
        Duration::from_millis((50u64.saturating_mul(3u64.saturating_pow(attempt as u32))).min(400))
    });
    tokio::time::sleep(delay.min(Duration::from_secs(5))).await;
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
    let statements = parse_batch_results(value)?;
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
            return Err(DbErr::Custom(format!(
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
fn parse_batch_results(value: &JsonValue) -> std::result::Result<Vec<DbPlanStmtExecResult>, DbErr> {
    let Some(arr) = value.get("result").and_then(JsonValue::as_array) else {
        return Err(ambiguous_d1("batch response missing result array"));
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, entry) in arr.iter().enumerate() {
        if entry.get("success").and_then(JsonValue::as_bool) == Some(false) {
            return Err(DbErr::Custom(format!(
                "D1 batch statement {i} failed: {entry}"
            )));
        }
        let rows = entry
            .get("results")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();
        let rows_affected = entry
            .get("meta")
            .and_then(|m| m.get("changes"))
            .and_then(JsonValue::as_u64)
            .unwrap_or(0);
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
        let value = json!({
            "result": [{ "success": false, "error": "constraint" }]
        });
        let err = parse_batch_results(&value).unwrap_err();
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
        let at_cap = json!({
            "result": [{ "success": true, "results": rows[..1000].to_vec(), "meta": { "changes": 0 } }]
        });
        let exec = parse_generic_batch(&plan, &at_cap, "op-cap-ok".into(), started).unwrap();
        assert_eq!(exec.statements[0].rows.len(), 1_000);
    }
}
