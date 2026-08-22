//! Generic D1 HTTP `batch()` executor for host-authored SQL plans.
//!
//! The guest does not parse Bookclerk operation names. The host compiles
//! domain work into [`DbAtomicPlan`]; this module runs that list as one
//! D1 `{ "batch": [...] }` SQL transaction and returns rows.

use std::time::Duration;

use bookclerk_plugin_sdk::{
    sea_null_kind, DbAtomicPlan, DbAtomicRequest, DbAtomicResult, DbAtomicTiming,
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
    ) -> std::result::Result<DbAtomicResult, DbErr> {
        let started = std::time::Instant::now();
        let plan = req
            .plan
            .clone()
            .ok_or_else(|| DbErr::Custom("dbAtomic requires a host-authored executePlan".into()))?;
        let expected_hash = req.request_hash.clone().unwrap_or_default();
        let statements: Vec<SqlStmt> = plan
            .statements
            .iter()
            .map(|s| (s.sql.clone(), d1_wire_binds(&s.binds)))
            .collect();
        let mut last_err = None;
        for attempt in 0..ATOMIC_HTTP_ATTEMPTS {
            let raw = match self.run_batch(&statements).await {
                Ok(value) => value,
                Err(err) if err.is_retryable() && attempt + 1 < ATOMIC_HTTP_ATTEMPTS => {
                    sleep_before_d1_retry(attempt, err.retry_after()).await;
                    last_err = Some(DbErr::from(err));
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
            match parse_generic_batch(&plan, &raw, &req.operation_id) {
                Ok(results) => {
                    let stmt_results: Vec<bookclerk_library::sql_plan::PlanStmtResult> = results
                        .into_iter()
                        .map(|r| bookclerk_library::sql_plan::PlanStmtResult { rows: r.rows })
                        .collect();
                    let mut result = bookclerk_library::sql_plan::interpret_plan(
                        &plan,
                        &stmt_results,
                        &expected_hash,
                    );
                    let db_execution_us = d1_sql_duration_us(&raw);
                    result.operation_id = req.operation_id;
                    result.timing = Some(DbAtomicTiming {
                        attempt_elapsed_us: u64::try_from(started.elapsed().as_micros())
                            .unwrap_or(u64::MAX),
                        db_execution_us,
                        db_timing_source: db_execution_us.map(|_| "d1_sql_duration".into()),
                    });
                    return Ok(result);
                }
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
/// client 4xx → `invalid_params`, other failures → `internal`.
#[must_use]
pub fn plugin_error_from_d1(err: DbErr) -> bookclerk_plugin_sdk::PluginError {
    if is_ambiguous_d1(&err) {
        return bookclerk_plugin_sdk::PluginError::unavailable(err.to_string());
    }
    let text = err.to_string();
    let lower = text.to_lowercase();
    if lower.contains("unique") || lower.contains("constraint") {
        return bookclerk_plugin_sdk::PluginError::conflict(text);
    }
    if let Some(status) = permanent_http_status(&err) {
        if (400..500).contains(&status) {
            return bookclerk_plugin_sdk::PluginError::invalid_params(text);
        }
    }
    bookclerk_plugin_sdk::PluginError::internal(text)
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

#[derive(Debug, Clone)]
/// Rows returned by one statement in a D1 HTTP batch response.
struct BatchStmtResult {
    /// Result rows for this statement; empty when the statement did not return rows.
    rows: Vec<JsonValue>,
}

/// Parses a D1 batch for a host-authored [`DbAtomicPlan`].
fn parse_generic_batch(
    plan: &DbAtomicPlan,
    value: &JsonValue,
    operation_id: &str,
) -> std::result::Result<Vec<BatchStmtResult>, DbErr> {
    let results = parse_batch_results(value)?;
    if results.len() != plan.statements.len() {
        return Err(ambiguous_d1(format!(
            "expected {} statement results, got {}",
            plan.statements.len(),
            results.len()
        )));
    }
    if let Some(idx) = plan.prior_receipt_index {
        let idx = idx as usize;
        if let Some(row) = results.get(idx).and_then(|r| r.rows.first()) {
            validate_receipt_row(row, operation_id)?;
        }
    }
    if let Some(idx) = plan.receipt_select_index {
        let idx = idx as usize;
        let Some(row) = results.get(idx).and_then(|r| r.rows.first()) else {
            return Err(ambiguous_d1("missing final receipt row"));
        };
        validate_receipt_row(row, operation_id)?;
    }
    Ok(results)
}

/// Reads a required non-empty string field from a receipt row, or marks the response ambiguous.
fn required_receipt_string<'a>(
    row: &'a JsonValue,
    field: &str,
) -> std::result::Result<&'a str, DbErr> {
    row.get(field)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ambiguous_d1(format!("malformed receipt row: missing {field}")))
}

/// Requires `operation_id`, `request_hash`, `status`, and `created_at`, and checks the id matches.
fn validate_receipt_row(
    row: &JsonValue,
    expected_operation_id: &str,
) -> std::result::Result<(), DbErr> {
    let op_id = required_receipt_string(row, "operation_id")?;
    let _hash = required_receipt_string(row, "request_hash")?;
    let _status = required_receipt_string(row, "status")?;
    let _created = required_receipt_string(row, "created_at")?;
    if op_id != expected_operation_id {
        return Err(ambiguous_d1(format!(
            "receipt operation_id {op_id} != {expected_operation_id}"
        )));
    }
    Ok(())
}

/// Parses the D1 `result` array; a `success: false` entry is a hard statement failure.
fn parse_batch_results(value: &JsonValue) -> std::result::Result<Vec<BatchStmtResult>, DbErr> {
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
        out.push(BatchStmtResult { rows });
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
        let err = parse_generic_batch(&plan, &json!({}), "op-1").unwrap_err();
        assert!(is_ambiguous_d1(&err), "{err}");
    }

    #[test]
    fn statement_failure_is_not_ambiguous() {
        let plan = DbAtomicPlan {
            statements: vec![],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let value = json!({
            "result": [{ "success": false, "error": "constraint" }]
        });
        // statements.len() is 0, result len is 1 → ambiguous count; use parse_batch_results
        let err = parse_batch_results(&value).unwrap_err();
        assert!(!is_ambiguous_d1(&err), "{err}");
        let _ = plan;
    }
}
