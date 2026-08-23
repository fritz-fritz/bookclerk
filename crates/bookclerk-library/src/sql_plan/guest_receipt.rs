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

/// Interprets a wrapped guest reply: conflict on hash mismatch, else strip wrapper rows.
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
    if let Some(prior_hash) = receipt_hash_from(&reply.statements[1]) {
        if prior_hash != guest_hash {
            return Err(PluginError::conflict(
                "executeAtomic operationId was already committed with a different requestHash",
            ));
        }
    }
    reply.statements = reply
        .statements
        .drain(WRAP_PREFIX..WRAP_PREFIX + guest_len)
        .collect();
    Ok(reply)
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
