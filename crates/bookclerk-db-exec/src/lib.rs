//! Neutral SQL executor for Bookclerk database guests.
//!
//! Hosts compile domain work into a generic [`host_ir::DbAtomicPlan`].
//! This crate runs those statements as one native transaction and records
//! fail-closed begin/commit faults. It must not import Bookclerk entities or
//! migrations.

use std::cell::RefCell;

use bookclerk_plugin_abi::DbColumn;

mod b64;
mod exec;
pub mod guest_receipt;
pub mod host_ir;
mod lower;
pub mod proxy_txn;
mod typed;

thread_local! {
    static POSITIONAL_COLUMNS: RefCell<Option<Vec<DbColumn>>> = const { RefCell::new(None) };
}

/// Records engine column metadata for the in-flight query (SQLite rusqlite path).
///
/// Prefers the request-scoped [`ExecBudget`] so metadata survives `spawn_blocking`.
pub fn set_positional_result_columns(columns: Vec<DbColumn>) {
    if let Some(budget) = crate::proxy_txn::current_exec_budget() {
        budget.set_positional_columns(columns);
        return;
    }
    POSITIONAL_COLUMNS.with(|slot| *slot.borrow_mut() = Some(columns));
}

/// Takes engine column metadata recorded by the SQLite adapter, if any.
#[must_use]
pub fn take_positional_result_columns() -> Option<Vec<DbColumn>> {
    if let Some(budget) = crate::proxy_txn::current_exec_budget() {
        if let Some(cols) = budget.take_positional_columns() {
            return Some(cols);
        }
    }
    POSITIONAL_COLUMNS.with(|slot| slot.borrow_mut().take())
}

pub use b64::{b64_string_to_bytes, bytes_to_b64_string};
pub use bookclerk_plugin_abi::DbPlanStatementKind;
pub use exec::{
    cap_query_sql, encoded_proxy_row_len, execute_statements_on, execute_statements_on_session,
    json_cell_utf8_len, note_encoded_result_bytes, sea_value_to_json, AtomicSession, ExecCaps,
};
pub use guest_receipt::{guest_receipt_finalize_stmts, GUEST_RECEIPT_WRAP_PREFIX};
pub use host_ir::{
    sea_null, sea_null_kind, DbAtomicPlan, DbAtomicRequest, DbAtomicTiming, DbPlanExecResult,
    DbPlanStatement, DbPlanStmtExecResult, DB_ATOMIC_SENTINEL, DB_CAPABILITIES_SENTINEL,
    SEA_NULL_KEY,
};
pub use lower::{lower_canonical_sql, lower_canonical_to_postgres};
pub use proxy_txn::{
    arm_exec_budget, clear_exec_budget, consume_atomic_interrupt, consume_begin_injection,
    consume_commit_injection, current_exec_budget, exec_deadline_expired,
    exec_deadline_remaining_ms, inject_atomic_interrupt, inject_atomic_interrupt_after,
    inject_begin_failures, inject_commit_failures, inject_savepoint_release_failures,
    inject_savepoint_rollback_failures, is_txn_broken, note_begin_failed, note_commit_failed,
    note_query_row, query_row_cap, query_rows_seen, record_query_rows_seen, take_txn_fault,
    txn_broken_err, with_exec_budget, AtomicInterruptKind, AtomicInterruptPhase, ExecBudget,
};
pub use typed::{
    db_value_from_sea, db_value_to_sea, execute_typed_on_session, execute_typed_on_session_then,
    execute_typed_on_txn,
};
