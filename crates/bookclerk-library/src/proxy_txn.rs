//! Fail-closed transaction injects re-exported from [`bookclerk_db_exec`].

pub use bookclerk_db_exec::{
    arm_exec_budget, clear_exec_budget, consume_atomic_interrupt, consume_begin_injection,
    consume_commit_injection, exec_deadline_expired, exec_deadline_remaining_ms,
    inject_atomic_interrupt, inject_atomic_interrupt_after, inject_begin_failures,
    inject_commit_failures, is_txn_broken, note_begin_failed, note_commit_failed, note_query_row,
    query_row_cap, query_rows_seen, take_txn_fault, txn_broken_err, AtomicInterruptKind,
    AtomicInterruptPhase,
};
