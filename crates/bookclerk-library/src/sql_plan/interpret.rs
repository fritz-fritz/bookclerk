//! Decode generic atomic-plan statement results into [`DbAtomicResult`].

use bookclerk_plugin_abi::{atomic_status, DbAtomicPlan, DbAtomicResult};
use serde_json::Value as JsonValue;

/// Rows produced by one plan statement.
#[derive(Debug, Clone, Default)]
pub struct PlanStmtResult {
    /// Result-set rows (empty for DML).
    pub rows: Vec<JsonValue>,
}

/// Maps statement rows onto a [`DbAtomicResult`], preferring receipt rows.
#[must_use]
pub fn interpret_plan(
    plan: &DbAtomicPlan,
    results: &[PlanStmtResult],
    expected_hash: &str,
) -> DbAtomicResult {
    if let Some(idx) = plan.prior_receipt_index {
        let idx = idx as usize;
        if let Some(row) = results.get(idx).and_then(|r| r.rows.first()) {
            return interpret_receipt(Some(row), expected_hash, true);
        }
    }
    if let Some(idx) = plan.receipt_select_index {
        let idx = idx as usize;
        return interpret_receipt(
            results.get(idx).and_then(|r| r.rows.first()),
            expected_hash,
            false,
        );
    }
    let Some(outcome) = results
        .get(plan.outcome_index as usize)
        .and_then(|r| r.rows.first())
    else {
        return DbAtomicResult::with_status(atomic_status::CLAIM_INVALID);
    };
    let status = outcome
        .get("status")
        .and_then(JsonValue::as_str)
        .unwrap_or(atomic_status::CLAIM_INVALID);
    if status != atomic_status::OK {
        return DbAtomicResult::with_status(status);
    }
    let Some(payload_index) = plan.payload_index else {
        return DbAtomicResult::ok_unit();
    };
    let Some(row) = results
        .get(payload_index as usize)
        .and_then(|r| r.rows.first())
    else {
        return DbAtomicResult::with_status(atomic_status::NOT_FOUND);
    };
    if let Some(payload) = row.get("payload") {
        return match decode_receipt_payload(Some(payload)) {
            Some(value) => DbAtomicResult::ok(value),
            None => DbAtomicResult::ok_unit(),
        };
    }
    DbAtomicResult::ok(row.clone())
}

/// Decodes a receipt row, flagging an idempotency conflict when `request_hash` differs.
fn interpret_receipt(
    row: Option<&JsonValue>,
    expected_hash: &str,
    replayed: bool,
) -> DbAtomicResult {
    let Some(row) = row else {
        return DbAtomicResult::with_status(atomic_status::EMPTY);
    };
    let stored_hash = row
        .get("request_hash")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    if stored_hash != expected_hash {
        return DbAtomicResult::with_status(atomic_status::IDEMPOTENCY_CONFLICT);
    }
    let status = row
        .get("status")
        .and_then(JsonValue::as_str)
        .unwrap_or(atomic_status::EMPTY);
    let created_at = row
        .get("created_at")
        .and_then(JsonValue::as_str)
        .map(str::to_string);
    let mut result = if status == atomic_status::OK {
        match decode_receipt_payload(row.get("payload")) {
            Some(payload) => DbAtomicResult::ok(payload),
            None => DbAtomicResult::ok_unit(),
        }
    } else {
        DbAtomicResult::with_status(status)
    };
    result.replayed = replayed;
    result.receipt_created_at = created_at;
    result
}

/// Parses a receipt `payload` cell, accepting JSON objects or a JSON-encoded string.
fn decode_receipt_payload(value: Option<&JsonValue>) -> Option<JsonValue> {
    match value {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::String(s)) => serde_json::from_str(s)
            .ok()
            .or_else(|| Some(JsonValue::String(s.clone()))),
        Some(other) => Some(other.clone()),
    }
}
