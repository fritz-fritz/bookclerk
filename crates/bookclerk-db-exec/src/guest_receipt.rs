//! Guest-typed durable receipt finalize (same-transaction payload capture).
//!
//! Host wrap lives in `bookclerk-library`; adapters run
//! [`guest_receipt_finalize_stmts`] in the same atomic transaction before COMMIT.

#![allow(clippy::missing_docs_in_private_items)]

use bookclerk_plugin_abi::{
    DbPlanStatementKind, DbResultSelection, DbValue, ExecuteReply, StatementResult,
    TypedDbStatement,
};
use sea_orm::DbErr;

/// Maximum UTF-8 bytes stored in `db_atomic_receipts.payload` for guest replay.
pub const GUEST_TYPED_REPLAY_PAYLOAD_MAX_BYTES: usize = 65536;

/// Error text when a receipt committed but the caller-visible payload was lost.
pub const GUEST_RECEIPT_RESULT_LOST: &str =
    "guest receipt committed without payload; original response lost";

/// True when `err` is [`GUEST_RECEIPT_RESULT_LOST`].
#[must_use]
pub fn is_guest_receipt_result_lost(err: &DbErr) -> bool {
    err.to_string().contains(GUEST_RECEIPT_RESULT_LOST)
}

/// Prune + prior-select prefix ahead of the guest statements.
pub const GUEST_RECEIPT_WRAP_PREFIX: usize = 2;

/// Same-batch receipt stub INSERT after gated guest statements.
pub const GUEST_RECEIPT_STUB_SUFFIX: usize = 1;

/// True when the prior-receipt SELECT returned a row.
#[must_use]
pub fn prior_receipt_exists(stmt: &StatementResult) -> bool {
    receipt_hash_from(stmt).is_some()
}

/// True when wrap prefix results show a prior receipt and guest work remains.
#[must_use]
pub fn should_skip_remaining_guest_work(results: &[StatementResult], total: usize) -> bool {
    results.len() == GUEST_RECEIPT_WRAP_PREFIX
        && total > GUEST_RECEIPT_WRAP_PREFIX
        && results.get(1).is_some_and(prior_receipt_exists)
}

/// Pads skipped guest + stub results so the wrap shape stays intact.
pub fn pad_skipped_guest_results(results: &mut Vec<StatementResult>, total: usize) {
    while results.len() < total {
        results.push(StatementResult::from_affected(0));
    }
}

/// Builds finalize statements to persist a guest reply before COMMIT.
///
/// Returns an empty list when the prior receipt row already carries payload.
///
/// # Errors
///
/// Returns [`DbErr`] when the envelope is malformed or the payload exceeds
/// [`GUEST_TYPED_REPLAY_PAYLOAD_MAX_BYTES`].
pub fn guest_receipt_finalize_stmts(
    partial: &ExecuteReply,
    guest_len: usize,
    guest_hash: &str,
) -> Result<Vec<TypedDbStatement>, DbErr> {
    let expected = GUEST_RECEIPT_WRAP_PREFIX
        .saturating_add(guest_len)
        .saturating_add(GUEST_RECEIPT_STUB_SUFFIX);
    if partial.statements.len() != expected {
        return Err(DbErr::Custom(format!(
            "guest atomic receipt wrap returned {} statements; expected {expected}",
            partial.statements.len()
        )));
    }
    if let Some(prior) = partial.statements.get(1) {
        if let Some(prior_hash) = receipt_hash_from(prior) {
            if prior_hash != guest_hash {
                // Guest statements must already have been skipped; unwrap
                // reports conflict. Do not emit finalize SQL.
                return Ok(Vec::new());
            }
            if receipt_payload_text(prior).is_some() {
                return Ok(Vec::new());
            }
            if is_gated_guest_replay(partial, prior)? {
                return Err(DbErr::Custom(GUEST_RECEIPT_RESULT_LOST.into()));
            }
            let guest_reply = guest_slice_reply(partial, guest_len)?;
            let payload = encode_guest_replay_payload(&guest_reply)?;
            return Ok(vec![typed_exec(
                "UPDATE db_atomic_receipts SET payload = ? WHERE operation_id = ? AND status = 'ok'",
                vec![
                    DbValue::Text(payload),
                    DbValue::Text(partial.operation_id.clone()),
                ],
            )]);
        }
    }
    let guest_reply = guest_slice_reply(partial, guest_len)?;
    let payload = encode_guest_replay_payload(&guest_reply)?;
    Ok(vec![typed_exec(
        "UPDATE db_atomic_receipts SET payload = ? WHERE operation_id = ? AND status = 'ok'",
        vec![
            DbValue::Text(payload),
            DbValue::Text(partial.operation_id.clone()),
        ],
    )])
}

/// True when a prior receipt row exists but the stub INSERT was gated off (replay after commit).
///
/// # Errors
///
/// Returns [`DbErr::Custom`] when the wrapped reply is missing the receipt stub suffix.
fn is_gated_guest_replay(partial: &ExecuteReply, prior: &StatementResult) -> Result<bool, DbErr> {
    if prior.rows.is_empty() {
        return Ok(false);
    }
    if receipt_payload_text(prior).is_some() {
        return Ok(false);
    }
    let stub = partial
        .statements
        .last()
        .ok_or_else(|| DbErr::Custom("guest receipt wrap missing stub suffix".into()))?;
    Ok(stub.rows_affected == 0)
}

/// Guest statement results from a wrapped partial reply.
///
/// # Errors
///
/// Never returns an error; the `Result` preserves the surrounding receipt API.
fn guest_slice_reply(partial: &ExecuteReply, guest_len: usize) -> Result<ExecuteReply, DbErr> {
    Ok(ExecuteReply {
        operation_id: partial.operation_id.clone(),
        statements: partial
            .statements
            .iter()
            .skip(GUEST_RECEIPT_WRAP_PREFIX)
            .take(guest_len)
            .cloned()
            .collect(),
        timing: partial.timing.clone(),
    })
}

/// Serializes guest statement results for durable replay.
///
/// # Errors
///
/// Returns when JSON serialization fails or the payload exceeds
/// [`GUEST_TYPED_REPLAY_PAYLOAD_MAX_BYTES`].
fn encode_guest_replay_payload(reply: &ExecuteReply) -> Result<String, DbErr> {
    let payload = GuestReplayPayload {
        operation_id: reply.operation_id.clone(),
        statements: reply.statements.clone(),
        timing: reply.timing.clone(),
    };
    let text = serde_json::to_string(&payload)
        .map_err(|err| DbErr::Custom(format!("guest replay payload encode failed: {err}")))?;
    if text.len() > GUEST_TYPED_REPLAY_PAYLOAD_MAX_BYTES {
        return Err(DbErr::Custom(format!(
            "guest replay payload is {} bytes; max is {}",
            text.len(),
            GUEST_TYPED_REPLAY_PAYLOAD_MAX_BYTES
        )));
    }
    Ok(text)
}

/// JSON envelope stored in `db_atomic_receipts.payload` for guest replay.
#[derive(serde::Serialize, serde::Deserialize)]
struct GuestReplayPayload {
    operation_id: String,
    statements: Vec<StatementResult>,
    timing: bookclerk_plugin_abi::DbTiming,
}

/// Reads `payload` from a prior-receipt select row, if present.
fn receipt_payload_text(prior: &StatementResult) -> Option<String> {
    let row = prior.rows.first()?;
    let idx = prior
        .columns
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case("payload"))
        .or(Some(3))?;
    match row.values.get(idx)? {
        DbValue::Text(s) if !s.is_empty() => Some(s.clone()),
        DbValue::Null(_) => None,
        _ => None,
    }
}

/// Reads `request_hash` from a prior-receipt select result, if any row exists.
fn receipt_hash_from(stmt: &StatementResult) -> Option<String> {
    let row = stmt.rows.first()?;
    let idx = stmt
        .columns
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case("request_hash"))
        .or(Some(1))?;
    match row.values.get(idx)? {
        DbValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

/// Host-authored execute statement used in the receipt envelope.
fn typed_exec(sql: &str, parameters: Vec<DbValue>) -> TypedDbStatement {
    TypedDbStatement {
        sql: sql.into(),
        parameters,
        kind: DbPlanStatementKind::Execute,
        max_rows: 0,
        result_selection: DbResultSelection::AffectedRows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookclerk_plugin_abi::{DbColumn, DbRow, DbTiming, DbType};

    fn prior_receipt_row(payload: &str) -> StatementResult {
        StatementResult {
            rows: vec![DbRow {
                values: vec![
                    DbValue::Text("guest-op".into()),
                    DbValue::Text("a".repeat(64)),
                    DbValue::Text("ok".into()),
                    DbValue::Text(payload.into()),
                    DbValue::Text("2026-01-01T00:00:00Z".into()),
                ],
            }],
            columns: vec![
                DbColumn {
                    name: "operation_id".into(),
                    db_type: DbType::Text,
                },
                DbColumn {
                    name: "request_hash".into(),
                    db_type: DbType::Text,
                },
                DbColumn {
                    name: "status".into(),
                    db_type: DbType::Text,
                },
                DbColumn {
                    name: "payload".into(),
                    db_type: DbType::Text,
                },
                DbColumn {
                    name: "created_at".into(),
                    db_type: DbType::Text,
                },
            ],
            rows_affected: 0,
        }
    }

    fn wrapped_partial(guest_rows_affected: u64, stub_rows_affected: u64) -> ExecuteReply {
        ExecuteReply {
            operation_id: "guest-op".into(),
            statements: vec![
                StatementResult::from_affected(0),
                prior_receipt_row(""),
                StatementResult::from_affected(guest_rows_affected),
                StatementResult::from_affected(stub_rows_affected),
            ],
            timing: DbTiming::default(),
        }
    }

    #[test]
    fn finalize_persists_payload_on_first_commit() {
        let partial = wrapped_partial(1, 1);
        let stmts = guest_receipt_finalize_stmts(&partial, 1, &"a".repeat(64)).expect("finalize");
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0]
            .sql
            .contains("UPDATE db_atomic_receipts SET payload"));
    }

    #[test]
    fn finalize_skips_sql_on_hash_mismatch() {
        let mut partial = wrapped_partial(1, 1);
        partial.statements[1] = prior_receipt_row("{\"operationId\":\"guest-op\"}");
        let stmts = guest_receipt_finalize_stmts(&partial, 1, &"b".repeat(64)).expect("mismatch");
        assert!(stmts.is_empty());
    }

    #[test]
    fn finalize_skips_when_payload_already_present() {
        let mut partial = wrapped_partial(1, 1);
        partial.statements[1] = prior_receipt_row("{\"operationId\":\"guest-op\"}");
        let stmts = guest_receipt_finalize_stmts(&partial, 1, &"a".repeat(64)).expect("finalize");
        assert!(stmts.is_empty());
    }

    #[test]
    fn finalize_rejects_gated_replay_without_payload() {
        let partial = wrapped_partial(0, 0);
        let err = guest_receipt_finalize_stmts(&partial, 1, &"a".repeat(64)).unwrap_err();
        assert!(err.to_string().contains("original response lost"), "{err}");
    }

    #[test]
    fn skip_remaining_guest_work_only_after_prior_select_row() {
        let empty_prefix = vec![
            StatementResult::from_affected(0),
            StatementResult::from_affected(0),
        ];
        assert!(!should_skip_remaining_guest_work(&empty_prefix, 4));
        let prior_prefix = vec![
            StatementResult::from_affected(0),
            prior_receipt_row("{\"operationId\":\"guest-op\"}"),
        ];
        assert!(should_skip_remaining_guest_work(&prior_prefix, 4));
        assert!(!should_skip_remaining_guest_work(&prior_prefix, 2));
    }
}
