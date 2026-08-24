//! Durable `(operationId, requestHash)` receipt wrap for guest-authored typed batches.
//!
//! Host domain plans already gate DML with `db_atomic_receipts`. Guest
//! `executeAtomic` must use the same envelope so a D1 (or any) adapter cannot
//! apply guest mutations twice after an ambiguous commit.

use bookclerk_plugin_abi::{
    DbPlanStatementKind, DbResultSelection, DbValue, ExecuteReply, ExecuteRequest, PluginError,
    TypedDbStatement,
};
use chrono::{Duration, Utc};

use super::named::apply_write_predicate;

/// Host `operationKind` stored on guest-typed receipts.
const GUEST_TYPED_KIND: &str = "guestTyped";

/// Maximum UTF-8 bytes stored in `db_atomic_receipts.payload` for guest replay.
pub const GUEST_TYPED_REPLAY_PAYLOAD_MAX_BYTES: usize = 65536;

/// Prune + prior-select prefix ahead of the guest statements.
const WRAP_PREFIX: usize = 2;
/// Receipt insert after the guest statements.
const WRAP_SUFFIX: usize = 1;

/// Wraps an already-authorized guest batch with prune / prior-select / gated
/// DML / receipt-insert. Clears `requestHash` so a later host re-authorize
/// stamps the wrapper, not the original guest SQL.
#[must_use]
pub(crate) fn wrap_guest_typed_request(mut req: ExecuteRequest) -> ExecuteRequest {
    let now = Utc::now();
    let created = now.to_rfc3339();
    let expires = (now + Duration::hours(24)).to_rfc3339();
    let operation_id = req.operation_id.clone();
    let request_hash = req.request_hash.clone();

    let prune = typed_exec(
        "DELETE FROM db_atomic_receipts WHERE expires_at <= ? AND operation_id != ?",
        vec![
            DbValue::Text(created.clone()),
            DbValue::Text(operation_id.clone()),
        ],
    );
    let select = typed_query(
        "SELECT operation_id, request_hash, status, payload, created_at \
         FROM db_atomic_receipts WHERE operation_id = ?",
        vec![DbValue::Text(operation_id.clone())],
    );
    let mut gated = Vec::with_capacity(req.statements.len());
    for mut stmt in req.statements {
        if is_write(stmt.kind) {
            stmt.sql = apply_write_predicate(
                &stmt.sql,
                stmt.kind,
                "NOT EXISTS (SELECT 1 FROM db_atomic_receipts WHERE operation_id = ?)",
            );
            stmt.parameters.push(DbValue::Text(operation_id.clone()));
        }
        gated.push(stmt);
    }
    let insert = typed_exec(
        "INSERT INTO db_atomic_receipts (\
            operation_id, operation_kind, request_hash, status, payload, created_at, expires_at\
         ) SELECT ?, ?, ?, 'ok', NULL, ?, ? \
           WHERE NOT EXISTS (SELECT 1 FROM db_atomic_receipts WHERE operation_id = ?)",
        vec![
            DbValue::Text(operation_id.clone()),
            DbValue::Text(GUEST_TYPED_KIND.into()),
            DbValue::Text(request_hash),
            DbValue::Text(created),
            DbValue::Text(expires),
            DbValue::Text(operation_id.clone()),
        ],
    );

    let mut statements = Vec::with_capacity(WRAP_PREFIX + gated.len() + WRAP_SUFFIX);
    statements.push(prune);
    statements.push(select);
    statements.extend(gated);
    statements.push(insert);

    req.statements = statements;
    req.request_hash.clear();
    req.prior_receipt_index = 1;
    req.has_prior_receipt_index = true;
    req.outcome_index = 0;
    req.payload_index = 0;
    req.has_payload_index = false;
    req.receipt_select_index = 0;
    req.has_receipt_select_index = false;
    req
}

/// Interprets a wrapped guest reply: replay from stored payload, else strip wrapper rows.
///
/// # Errors
///
/// Returns [`PluginError::conflict`] when a prior receipt exists with a
/// different hash, or [`PluginError::internal`] when the envelope is malformed.
pub(crate) fn unwrap_guest_typed_reply(
    mut reply: ExecuteReply,
    guest_len: usize,
    guest_hash: &str,
) -> Result<ExecuteReply, PluginError> {
    let expected = WRAP_PREFIX
        .saturating_add(guest_len)
        .saturating_add(WRAP_SUFFIX);
    if reply.statements.len() != expected {
        return Err(PluginError::internal(format!(
            "guest atomic receipt wrap returned {} statements; expected {expected}",
            reply.statements.len()
        )));
    }
    if let Some(prior) = reply.statements.get(1) {
        if let Some(prior_hash) = receipt_hash_from(prior) {
            if prior_hash != guest_hash {
                return Err(PluginError::conflict(
                    "executeAtomic operationId was already committed with a different requestHash",
                ));
            }
            if let Some(replayed) = decode_guest_replay_payload(prior)? {
                return Ok(replayed);
            }
        }
    }
    reply.statements = reply
        .statements
        .drain(WRAP_PREFIX..WRAP_PREFIX + guest_len)
        .collect();
    Ok(reply)
}

/// Builds follow-up statements to persist a guest reply for idempotent replay.
///
/// Returns an empty list when the prior receipt row already carries payload.
///
/// # Errors
///
/// Returns [`DbErr`] when the envelope is malformed or the payload exceeds
/// [`GUEST_TYPED_REPLAY_PAYLOAD_MAX_BYTES`].
pub(crate) fn guest_receipt_persist_stmts(
    partial: &ExecuteReply,
    guest_len: usize,
    guest_hash: &str,
) -> Result<Vec<TypedDbStatement>, sea_orm::DbErr> {
    let expected = WRAP_PREFIX.saturating_add(guest_len).saturating_add(WRAP_SUFFIX);
    if partial.statements.len() != expected {
        return Err(sea_orm::DbErr::Custom(format!(
            "guest atomic receipt wrap returned {} statements; expected {expected}",
            partial.statements.len()
        )));
    }
    if let Some(prior) = partial.statements.get(1) {
        if let Some(prior_hash) = receipt_hash_from(prior) {
            if prior_hash != guest_hash {
                return Err(sea_orm::DbErr::Custom(
                    "executeAtomic operationId was already committed with a different requestHash"
                        .into(),
                ));
            }
            if receipt_payload_text(prior).is_some() {
                return Ok(Vec::new());
            }
        }
    }
    let guest_reply = ExecuteReply {
        operation_id: partial.operation_id.clone(),
        statements: partial
            .statements
            .iter()
            .skip(WRAP_PREFIX)
            .take(guest_len)
            .cloned()
            .collect(),
        timing: partial.timing.clone(),
    };
    let payload = encode_guest_replay_payload(&guest_reply)?;
    Ok(vec![typed_exec(
        "UPDATE db_atomic_receipts SET payload = ? WHERE operation_id = ? AND status = 'ok'",
        vec![
            DbValue::Text(payload),
            DbValue::Text(partial.operation_id.clone()),
        ],
    )])
}

/// Reconstructs a guest [`ExecuteReply`] from a prior-receipt select row.
fn decode_guest_replay_payload(
    prior: &bookclerk_plugin_abi::StatementResult,
) -> Result<Option<ExecuteReply>, PluginError> {
    let Some(text) = receipt_payload_text(prior) else {
        return Ok(None);
    };
    let payload: GuestReplayPayload = serde_json::from_str(&text).map_err(|err| {
        PluginError::internal(format!("guest replay payload is not valid JSON: {err}"))
    })?;
    Ok(Some(ExecuteReply {
        operation_id: payload.operation_id,
        statements: payload.statements,
        timing: payload.timing,
    }))
}

/// Serializes guest statement results for durable replay.
fn encode_guest_replay_payload(reply: &ExecuteReply) -> Result<String, sea_orm::DbErr> {
    let payload = GuestReplayPayload {
        operation_id: reply.operation_id.clone(),
        statements: reply.statements.clone(),
        timing: reply.timing.clone(),
    };
    let text = serde_json::to_string(&payload).map_err(|err| {
        sea_orm::DbErr::Custom(format!("guest replay payload encode failed: {err}"))
    })?;
    if text.len() > GUEST_TYPED_REPLAY_PAYLOAD_MAX_BYTES {
        return Err(sea_orm::DbErr::Custom(format!(
            "guest replay payload is {} bytes; max is {}",
            text.len(),
            GUEST_TYPED_REPLAY_PAYLOAD_MAX_BYTES
        )));
    }
    Ok(text)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GuestReplayPayload {
    operation_id: String,
    statements: Vec<bookclerk_plugin_abi::StatementResult>,
    timing: bookclerk_plugin_abi::DbTiming,
}

/// Reads `payload` from a prior-receipt select row, if present.
fn receipt_payload_text(prior: &bookclerk_plugin_abi::StatementResult) -> Option<String> {
    let row = prior.rows.first()?;
    let idx = prior
        .columns
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case("payload"))
        .or(Some(4))?;
    match row.values.get(idx)? {
        DbValue::Text(s) if !s.is_empty() => Some(s.clone()),
        DbValue::Null(_) => None,
        _ => None,
    }
}

/// True for DML kinds that must be receipt-gated.
fn is_write(kind: DbPlanStatementKind) -> bool {
    matches!(
        kind,
        DbPlanStatementKind::Execute | DbPlanStatementKind::Returning
    )
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

/// Host-authored select used to read a prior receipt row.
fn typed_query(sql: &str, parameters: Vec<DbValue>) -> TypedDbStatement {
    TypedDbStatement {
        sql: sql.into(),
        parameters,
        kind: DbPlanStatementKind::Select,
        max_rows: 1,
        result_selection: DbResultSelection::Rows,
    }
}

/// Reads `request_hash` from a prior-receipt select result, if any row exists.
fn receipt_hash_from(stmt: &bookclerk_plugin_abi::StatementResult) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use bookclerk_plugin_abi::DbPlanStatementKind;

    fn guest_insert() -> ExecuteRequest {
        ExecuteRequest {
            operation_id: "guest-op".into(),
            request_hash: "a".repeat(64),
            statements: vec![TypedDbStatement {
                sql: "INSERT INTO counters (id, n) VALUES (?, ?)".into(),
                parameters: vec![DbValue::Int64(1), DbValue::Int64(1)],
                kind: DbPlanStatementKind::Execute,
                max_rows: 0,
                result_selection: DbResultSelection::AffectedRows,
            }],
            outcome_index: 0,
            payload_index: 0,
            has_payload_index: false,
            prior_receipt_index: 0,
            has_prior_receipt_index: false,
            receipt_select_index: 0,
            has_receipt_select_index: false,
            deadline_unix_ms: 0,
        }
    }

    #[test]
    fn wrap_rewrites_insert_values_and_gates_writes() {
        let wrapped = wrap_guest_typed_request(guest_insert());
        assert!(wrapped.request_hash.is_empty());
        assert!(wrapped.has_prior_receipt_index);
        assert_eq!(wrapped.prior_receipt_index, 1);
        assert_eq!(wrapped.statements.len(), 4);
        let gated = &wrapped.statements[2];
        assert!(
            gated.sql.to_ascii_uppercase().contains("SELECT"),
            "INSERT VALUES must become INSERT SELECT: {}",
            gated.sql
        );
        assert!(
            !gated.sql.to_ascii_uppercase().contains("VALUES"),
            "{}",
            gated.sql
        );
        assert!(
            gated
                .sql
                .contains("NOT EXISTS (SELECT 1 FROM db_atomic_receipts"),
            "{}",
            gated.sql
        );
        assert_eq!(gated.parameters.len(), 3);
    }

    #[test]
    fn unwrap_replays_stored_guest_reply_on_hash_match() {
        let guest = guest_insert();
        let payload = encode_guest_replay_payload(&ExecuteReply {
            operation_id: guest.operation_id.clone(),
            statements: vec![bookclerk_plugin_abi::StatementResult::from_affected(1)],
            timing: bookclerk_plugin_abi::DbTiming::default(),
        })
        .expect("encode");
        let prior = bookclerk_plugin_abi::StatementResult {
            rows: vec![bookclerk_plugin_abi::DbRow {
                values: vec![
                    DbValue::Text("guest-op".into()),
                    DbValue::Text("a".repeat(64)),
                    DbValue::Text("ok".into()),
                    DbValue::Text(payload),
                    DbValue::Text("2026-01-01T00:00:00Z".into()),
                ],
            }],
            columns: vec![
                bookclerk_plugin_abi::DbColumn {
                    name: "operation_id".into(),
                    db_type: bookclerk_plugin_abi::DbType::Text,
                },
                bookclerk_plugin_abi::DbColumn {
                    name: "request_hash".into(),
                    db_type: bookclerk_plugin_abi::DbType::Text,
                },
                bookclerk_plugin_abi::DbColumn {
                    name: "status".into(),
                    db_type: bookclerk_plugin_abi::DbType::Text,
                },
                bookclerk_plugin_abi::DbColumn {
                    name: "payload".into(),
                    db_type: bookclerk_plugin_abi::DbType::Text,
                },
                bookclerk_plugin_abi::DbColumn {
                    name: "created_at".into(),
                    db_type: bookclerk_plugin_abi::DbType::Text,
                },
            ],
            rows_affected: 0,
            cursor: String::new(),
        };
        let reply = ExecuteReply {
            operation_id: "guest-op".into(),
            statements: vec![
                bookclerk_plugin_abi::StatementResult::from_affected(0),
                prior,
                bookclerk_plugin_abi::StatementResult::from_affected(0),
                bookclerk_plugin_abi::StatementResult::from_affected(0),
            ],
            timing: bookclerk_plugin_abi::DbTiming::default(),
        };
        let replayed = unwrap_guest_typed_reply(reply, 1, &"a".repeat(64)).expect("replay");
        assert_eq!(replayed.statements.len(), 1);
        assert_eq!(replayed.statements[0].rows_affected, 1);
    }

    #[test]
    fn unwrap_conflict_on_hash_mismatch() {
        let prior = bookclerk_plugin_abi::StatementResult {
            rows: vec![bookclerk_plugin_abi::DbRow {
                values: vec![
                    DbValue::Text("guest-op".into()),
                    DbValue::Text("other-hash".into()),
                ],
            }],
            columns: vec![
                bookclerk_plugin_abi::DbColumn {
                    name: "operation_id".into(),
                    db_type: bookclerk_plugin_abi::DbType::Text,
                },
                bookclerk_plugin_abi::DbColumn {
                    name: "request_hash".into(),
                    db_type: bookclerk_plugin_abi::DbType::Text,
                },
            ],
            rows_affected: 0,
            cursor: String::new(),
        };
        let reply = ExecuteReply {
            operation_id: "guest-op".into(),
            statements: vec![
                bookclerk_plugin_abi::StatementResult::from_affected(0),
                prior,
                bookclerk_plugin_abi::StatementResult::from_affected(0),
                bookclerk_plugin_abi::StatementResult::from_affected(0),
            ],
            timing: bookclerk_plugin_abi::DbTiming::default(),
        };
        let err = unwrap_guest_typed_reply(reply, 1, "expected-hash").unwrap_err();
        assert_eq!(err.code, bookclerk_plugin_abi::PluginErrorCode::Conflict);
    }
}
