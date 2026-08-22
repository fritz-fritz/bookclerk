//! Neutral SQL executor for Bookclerk database guests.
//!
//! Hosts compile domain work into a generic [`bookclerk_plugin_abi::DbAtomicPlan`].
//! This crate runs those statements as one native transaction and records
//! fail-closed begin/commit faults. It must not import Bookclerk entities or
//! migrations.

mod b64;
mod exec;
pub mod proxy_txn;

pub use b64::{b64_string_to_bytes, bytes_to_b64_string};
pub use exec::{
    cap_query_sql, execute_statements_on, execute_statements_on_session, AtomicSession,
};
pub use proxy_txn::{
    arm_exec_budget, clear_exec_budget, consume_atomic_interrupt, consume_begin_injection,
    consume_commit_injection, current_exec_budget, exec_deadline_expired,
    exec_deadline_remaining_ms, inject_atomic_interrupt, inject_atomic_interrupt_after,
    inject_begin_failures, inject_commit_failures, is_txn_broken, note_begin_failed,
    note_commit_failed, note_query_row, query_row_cap, query_rows_seen, record_query_rows_seen,
    take_txn_fault, txn_broken_err, with_exec_budget, AtomicInterruptKind, AtomicInterruptPhase,
    ExecBudget,
};
