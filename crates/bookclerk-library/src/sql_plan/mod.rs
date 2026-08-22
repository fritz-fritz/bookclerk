//! Host-owned SQL atomic plans for database plugins.
//!
//! The library compiles Bookclerk domain operations into a generic
//! [`bookclerk_plugin_abi::DbAtomicPlan`]. Database guests execute the
//! statements as one transaction and return rows; they must not parse
//! domain operation names.

#[cfg(test)]
mod conformance;
pub mod dialect;
mod exec;
mod interpret;
mod named;
mod slots;
pub mod vectors;

use bookclerk_plugin_abi::{
    DbAtomicPlan, DbAtomicRequest, DbConnectResult, DbPlanStatement, DbPlanStatementKind,
};

pub use dialect::{rewrite_placeholders, SqlFamily};
pub use exec::{
    execute_plan_on, execute_plan_on_capped, execute_statements_on, execute_statements_on_session,
    AtomicSession,
};
pub use interpret::{interpret_exec, interpret_plan, validate_exec_result, PlanStmtResult};
pub use named::{compile_claim_event_delivery, compile_named_request};
pub use slots::{event_inflight_slot, lock_serialization_slot, JOB_QUEUE_SLOT};

/// Compiled plan plus the hash stored on the receipt.
#[derive(Debug, Clone)]
pub struct CompiledAtomic {
    /// Wire plan executed by the guest.
    pub plan: DbAtomicPlan,
    /// SHA-256 hex compared on receipt replay.
    pub expected_hash: String,
}

impl CompiledAtomic {
    /// Envelope sent on `bookclerk.atomic`.
    #[must_use]
    pub fn into_request(self, operation_id: impl Into<String>) -> DbAtomicRequest {
        DbAtomicRequest::with_plan(operation_id, self.expected_hash, self.plan)
    }
}

/// SQL family from a negotiated [`DbConnectResult`].
#[must_use]
pub fn family_from_connect(caps: &DbConnectResult) -> Option<SqlFamily> {
    SqlFamily::parse(&caps.sql_family)
}

/// Rejects a plan that exceeds negotiated guest limits or has out-of-range selectors.
///
/// # Errors
///
/// Returns [`crate::LibraryError::Other`] when the plan cannot be sent.
pub fn validate_plan(plan: &DbAtomicPlan, caps: &DbConnectResult) -> crate::error::Result<()> {
    let n_stmt = u32::try_from(plan.statements.len()).unwrap_or(u32::MAX);
    if caps.max_statements > 0 && n_stmt > caps.max_statements {
        return Err(crate::LibraryError::Other(anyhow::anyhow!(
            "atomic plan has {n_stmt} statements; guest maxStatements is {}",
            caps.max_statements
        )));
    }
    let stmt_len = plan.statements.len();
    let mut selectors = vec![("outcomeIndex", plan.outcome_index)];
    if let Some(idx) = plan.payload_index {
        selectors.push(("payloadIndex", idx));
    }
    if let Some(idx) = plan.prior_receipt_index {
        selectors.push(("priorReceiptIndex", idx));
    }
    if let Some(idx) = plan.receipt_select_index {
        selectors.push(("receiptSelectIndex", idx));
    }
    for (name, idx) in selectors {
        if (idx as usize) >= stmt_len {
            return Err(crate::LibraryError::Other(anyhow::anyhow!(
                "atomic plan {name} {idx} is out of range for {stmt_len} statements"
            )));
        }
    }
    for (i, stmt) in plan.statements.iter().enumerate() {
        let n_binds = u32::try_from(stmt.binds.len()).unwrap_or(u32::MAX);
        if caps.max_binds > 0 && n_binds > caps.max_binds {
            return Err(crate::LibraryError::Other(anyhow::anyhow!(
                "atomic plan statement {i} has {n_binds} binds; guest maxBinds is {}",
                caps.max_binds
            )));
        }
        if caps.max_payload_bytes > 0 {
            let binds_len = serde_json::to_vec(&stmt.binds)
                .map(|b| b.len())
                .unwrap_or(0);
            let payload = stmt.sql.len().saturating_add(binds_len);
            let cap = usize::try_from(caps.max_payload_bytes).unwrap_or(usize::MAX);
            if payload > cap {
                return Err(crate::LibraryError::Other(anyhow::anyhow!(
                    "atomic plan statement {i} encoded payload is {payload} bytes; guest maxPayloadBytes is {}",
                    caps.max_payload_bytes
                )));
            }
        }
    }
    Ok(())
}

/// Wake page size from negotiated `maxBinds` (SET/EXISTS overhead is 4 binds).
#[must_use]
pub fn wake_page_for_max_binds(max_binds: u32) -> u64 {
    const FIXED: u32 = 4;
    u64::from(max_binds.saturating_sub(FIXED).clamp(8, 256))
}

/// Converts an internal statement list into the wire plan.
fn wire_plan(
    statements: Vec<(String, Vec<serde_json::Value>)>,
    outcome_index: usize,
    payload_index: Option<usize>,
    prior_receipt_index: Option<usize>,
    receipt_select_index: Option<usize>,
) -> DbAtomicPlan {
    DbAtomicPlan {
        statements: statements
            .into_iter()
            .map(|(sql, binds)| {
                let kind = host_statement_kind(&sql);
                DbPlanStatement { sql, binds, kind }
            })
            .collect(),
        outcome_index: u32::try_from(outcome_index).unwrap_or(0),
        payload_index: payload_index.and_then(|i| u32::try_from(i).ok()),
        prior_receipt_index: prior_receipt_index.and_then(|i| u32::try_from(i).ok()),
        receipt_select_index: receipt_select_index.and_then(|i| u32::try_from(i).ok()),
    }
}

/// Host-authored statement shape for the wire `kind` field.
///
/// Adapters must not reparse SQL; they trust this classification.
pub(crate) fn host_statement_kind(sql: &str) -> DbPlanStatementKind {
    let compact = sql
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    if compact.contains(" RETURNING ") || compact.ends_with(" RETURNING") {
        return DbPlanStatementKind::Returning;
    }
    match compact.split_whitespace().next().unwrap_or("") {
        "SELECT" | "WITH" | "VALUES" => DbPlanStatementKind::Select,
        _ => DbPlanStatementKind::Execute,
    }
}

#[cfg(test)]
mod limits_tests {
    use super::{
        validate_plan, DbAtomicPlan, DbConnectResult, DbPlanStatement, DbPlanStatementKind,
    };

    fn tiny_caps() -> DbConnectResult {
        let mut caps = DbConnectResult::sqlite();
        caps.max_statements = 2;
        caps.max_binds = 2;
        caps.max_payload_bytes = 64;
        caps
    }

    fn stmt(sql: &str, binds: Vec<serde_json::Value>) -> DbPlanStatement {
        DbPlanStatement {
            sql: sql.into(),
            binds,
            kind: DbPlanStatementKind::Query,
        }
    }

    #[test]
    fn validate_plan_rejects_too_many_statements() {
        let plan = DbAtomicPlan {
            statements: vec![
                stmt("SELECT 1", vec![]),
                stmt("SELECT 2", vec![]),
                stmt("SELECT 3", vec![]),
            ],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let err = validate_plan(&plan, &tiny_caps()).unwrap_err();
        assert!(err.to_string().contains("maxStatements"), "{err}");
    }

    #[test]
    fn validate_plan_rejects_too_many_binds() {
        let plan = DbAtomicPlan {
            statements: vec![stmt(
                "SELECT ?, ?, ?",
                vec![
                    serde_json::json!(1),
                    serde_json::json!(2),
                    serde_json::json!(3),
                ],
            )],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let err = validate_plan(&plan, &tiny_caps()).unwrap_err();
        assert!(err.to_string().contains("maxBinds"), "{err}");
    }

    #[test]
    fn validate_plan_rejects_payload_bytes() {
        let plan = DbAtomicPlan {
            statements: vec![stmt("SELECT ?", vec![serde_json::json!("x".repeat(200))])],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let err = validate_plan(&plan, &tiny_caps()).unwrap_err();
        assert!(err.to_string().contains("maxPayloadBytes"), "{err}");
    }

    #[test]
    fn validate_plan_rejects_oversized_dispatch_page() {
        use crate::atomic_ops::DbAtomicParams;
        use crate::sql_plan::{compile_named_request, SqlFamily};
        let subs: Vec<serde_json::Value> = (0..50)
            .map(|i| serde_json::json!({ "pluginId": format!("p{i}") }))
            .collect();
        let compiled = compile_named_request(
            "oversize",
            &DbAtomicParams::DispatchEventDeliveries {
                event_id: "evt".into(),
                subscribers_json: serde_json::to_string(&subs).unwrap(),
                mark_dispatched: true,
            },
            "2024-06-01T00:00:00Z",
            SqlFamily::Sqlite,
        )
        .unwrap();
        let mut caps = DbConnectResult::sqlite();
        caps.max_statements = 40;
        let err = validate_plan(&compiled.plan, &caps).unwrap_err();
        assert!(err.to_string().contains("maxStatements"), "{err}");
        assert!(
            compiled.plan.statements.len() > 40,
            "planner must emit inserts for every subscriber, not take(24)"
        );
    }

    #[test]
    fn validate_plan_rejects_out_of_range_selector() {
        let plan = DbAtomicPlan {
            statements: vec![stmt("SELECT 1", vec![])],
            outcome_index: 3,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let err = validate_plan(&plan, &DbConnectResult::sqlite()).unwrap_err();
        assert!(err.to_string().contains("outcomeIndex"), "{err}");
    }

    #[test]
    fn host_kind_marks_cte_insert_returning_not_select() {
        let sql = "WITH seed AS (SELECT 1) INSERT INTO t SELECT * FROM seed RETURNING id";
        assert_eq!(
            super::host_statement_kind(sql),
            DbPlanStatementKind::Returning
        );
        assert!(!DbPlanStatementKind::Returning.wrap_select_limit());
        assert_eq!(
            super::host_statement_kind("SELECT x FROM t"),
            DbPlanStatementKind::Select
        );
        assert_eq!(
            super::host_statement_kind(
                "WITH RECURSIVE t(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM t WHERE x < 3) SELECT x FROM t"
            ),
            DbPlanStatementKind::Select
        );
    }
}
