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

use bookclerk_plugin_abi::{
    DbAtomicPlan, DbAtomicRequest, DbConnectResult, DbPlanStatement, DbPlanStatementKind,
};

pub use dialect::{rewrite_placeholders, SqlFamily};
pub use exec::execute_plan_on;
pub use interpret::{interpret_plan, PlanStmtResult};
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
                let kind = statement_kind(&sql);
                DbPlanStatement { sql, binds, kind }
            })
            .collect(),
        outcome_index: u32::try_from(outcome_index).unwrap_or(0),
        payload_index: payload_index.and_then(|i| u32::try_from(i).ok()),
        prior_receipt_index: prior_receipt_index.and_then(|i| u32::try_from(i).ok()),
        receipt_select_index: receipt_select_index.and_then(|i| u32::try_from(i).ok()),
    }
}

/// Treats `SELECT` and DML with `RETURNING` as row-producing statements.
fn statement_kind(sql: &str) -> DbPlanStatementKind {
    let upper = sql.trim_start().to_ascii_uppercase();
    if upper.starts_with("SELECT") || upper.contains(" RETURNING ") {
        DbPlanStatementKind::Query
    } else {
        DbPlanStatementKind::Execute
    }
}
