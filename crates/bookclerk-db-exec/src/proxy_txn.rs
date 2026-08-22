//! Fail-closed acknowledgements for SeaORM's infallible proxy transaction hooks.
//!
//! `ProxyDatabaseTrait::{begin, commit, rollback}` return `()`. A failed
//! `BEGIN` that is only logged leaves later statements in autocommit; a failed
//! `COMMIT` still looks like success to [`sea_orm::DatabaseTransaction::commit`].
//! Proxies record a per-task fault instead and refuse subsequent statements.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::thread::ThreadId;
use std::time::{SystemTime, UNIX_EPOCH};

use sea_orm::DbErr;
use tokio::task::{try_id, Id as TaskId};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
/// Per-task identity used to pin sticky begin/commit faults.
enum TaskKey {
    /// Tokio task id when running inside the runtime.
    Tokio(TaskId),
    /// OS thread id when no Tokio task is available.
    Thread(ThreadId),
}

/// Current task key: Tokio id if present, otherwise the OS thread.
fn task_key() -> TaskKey {
    match try_id() {
        Some(id) => TaskKey::Tokio(id),
        None => TaskKey::Thread(std::thread::current().id()),
    }
}

/// Sticky begin/commit fault messages keyed by task (fail-closed after a hook error).
static FAULTS: LazyLock<Mutex<HashMap<TaskKey, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Remaining injected `BEGIN` failures for tests, keyed by task.
static INJECT_BEGIN: LazyLock<Mutex<HashMap<TaskKey, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Remaining injected `COMMIT` failures for tests, keyed by task.
static INJECT_COMMIT: LazyLock<Mutex<HashMap<TaskKey, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Injected atomic session interrupt (cancel or deadline) for tests.
static INJECT_INTERRUPT: LazyLock<Mutex<HashMap<TaskKey, InjectedInterrupt>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Guest-visible atomic deadline (`0` = none). Read from rusqlite progress handlers.
static EXEC_DEADLINE_UNIX_MS: AtomicU64 = AtomicU64::new(0);
/// Advertised query row cap (`0` = unlimited). Callers stop after `cap + 1`.
static QUERY_ROW_CAP: AtomicUsize = AtomicUsize::new(0);
/// Rows materialized by the current capped query (tests / early-stop).
static QUERY_ROWS_SEEN: AtomicUsize = AtomicUsize::new(0);

/// Phase at which an injected atomic interrupt fires.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtomicInterruptPhase {
    /// Before `BEGIN` / HTTP batch.
    BeforeBegin,
    /// Between statements during the transaction.
    BetweenStatements,
    /// Around `COMMIT` / HTTP return (ambiguous).
    AroundCommit,
}

/// Kind of injected atomic interrupt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtomicInterruptKind {
    /// Session cancel.
    Cancel,
    /// Deadline elapsed.
    Deadline,
}

/// One test-injected interrupt.
#[derive(Clone, Copy, Debug)]
struct InjectedInterrupt {
    /// Phase that should observe the interrupt.
    phase: AtomicInterruptPhase,
    /// Cancel vs deadline.
    kind: AtomicInterruptKind,
    /// Times this phase is observed before the interrupt fires.
    skip: u32,
}

/// Locks the fault map, recovering a poisoned mutex so fail-closed still works.
fn lock_faults() -> std::sync::MutexGuard<'static, HashMap<TaskKey, String>> {
    FAULTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Record that `BEGIN` failed so later statements cannot autocommit.
pub fn note_begin_failed(err: impl std::fmt::Display) {
    lock_faults().insert(task_key(), format!("database begin failed: {err}"));
}

/// Record that `COMMIT` failed so the caller cannot treat the txn as durable.
pub fn note_commit_failed(err: impl std::fmt::Display) {
    lock_faults().insert(task_key(), format!("database commit failed: {err}"));
}

/// Whether this task has a sticky begin/commit fault.
#[must_use]
pub fn is_txn_broken() -> bool {
    lock_faults().contains_key(&task_key())
}

/// `DbErr` for refusing statements after a failed begin/commit.
#[must_use]
pub fn txn_broken_err() -> DbErr {
    let msg = lock_faults()
        .get(&task_key())
        .cloned()
        .unwrap_or_else(|| "database transaction is broken".into());
    DbErr::Custom(msg)
}

/// Take and clear this task's sticky fault (after SeaORM commit/rollback).
#[must_use]
pub fn take_txn_fault() -> Option<String> {
    lock_faults().remove(&task_key())
}

/// Queue `n` injected `BEGIN` failures for this task.
pub fn inject_begin_failures(n: u32) {
    INJECT_BEGIN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(task_key(), n);
}

/// Queue `n` injected `COMMIT` failures for this task.
pub fn inject_commit_failures(n: u32) {
    INJECT_COMMIT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(task_key(), n);
}

/// Consume one injected begin failure if any remain for this task.
#[must_use]
pub fn consume_begin_injection() -> bool {
    consume_injection(&INJECT_BEGIN)
}

/// Consume one injected commit failure if any remain for this task.
#[must_use]
pub fn consume_commit_injection() -> bool {
    consume_injection(&INJECT_COMMIT)
}

/// Queue an atomic session interrupt for this task (cancel or deadline at `phase`).
pub fn inject_atomic_interrupt(phase: AtomicInterruptPhase, kind: AtomicInterruptKind) {
    inject_atomic_interrupt_after(phase, kind, 0);
}

/// Like [`inject_atomic_interrupt`], ignoring `skip` observations of `phase` first.
pub fn inject_atomic_interrupt_after(
    phase: AtomicInterruptPhase,
    kind: AtomicInterruptKind,
    skip: u32,
) {
    INJECT_INTERRUPT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(task_key(), InjectedInterrupt { phase, kind, skip });
}

/// Consume an injected interrupt when the executor reaches `phase`.
#[must_use]
pub fn consume_atomic_interrupt(phase: AtomicInterruptPhase) -> Option<AtomicInterruptKind> {
    let mut map = INJECT_INTERRUPT.lock().unwrap_or_else(|e| e.into_inner());
    let key = task_key();
    match map.get_mut(&key).copied() {
        Some(inj) if inj.phase == phase => {
            if inj.skip > 0 {
                if let Some(slot) = map.get_mut(&key) {
                    slot.skip -= 1;
                }
                None
            } else {
                map.remove(&key);
                Some(inj.kind)
            }
        }
        _ => None,
    }
}

/// Decrements one injected failure for this task; returns true when a fault should fire.
fn consume_injection(map: &Mutex<HashMap<TaskKey, u32>>) -> bool {
    let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
    let key = task_key();
    match map.get_mut(&key) {
        Some(n) if *n > 0 => {
            *n -= 1;
            if *n == 0 {
                map.remove(&key);
            }
            true
        }
        _ => false,
    }
}

/// Unix time in milliseconds.
fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Arms engine-level deadline and query row cap for the current atomic attempt.
pub fn arm_exec_budget(deadline_unix_ms: Option<u64>, query_row_cap: u32) {
    EXEC_DEADLINE_UNIX_MS.store(deadline_unix_ms.unwrap_or(0), Ordering::SeqCst);
    QUERY_ROW_CAP.store(
        usize::try_from(query_row_cap).unwrap_or(0),
        Ordering::SeqCst,
    );
    QUERY_ROWS_SEEN.store(0, Ordering::SeqCst);
}

/// Clears engine-level deadline and query row cap.
pub fn clear_exec_budget() {
    EXEC_DEADLINE_UNIX_MS.store(0, Ordering::SeqCst);
    QUERY_ROW_CAP.store(0, Ordering::SeqCst);
}

/// True when an armed deadline has elapsed (rusqlite progress handler).
#[must_use]
pub fn exec_deadline_expired() -> bool {
    let dl = EXEC_DEADLINE_UNIX_MS.load(Ordering::SeqCst);
    dl > 0 && unix_now_ms() >= dl
}

/// Remaining milliseconds until the armed deadline (`None` if unlimited).
#[must_use]
pub fn exec_deadline_remaining_ms() -> Option<u64> {
    let dl = EXEC_DEADLINE_UNIX_MS.load(Ordering::SeqCst);
    if dl == 0 {
        return None;
    }
    Some(dl.saturating_sub(unix_now_ms()).max(1))
}

/// Advertised query row cap (`None` if unlimited).
#[must_use]
pub fn query_row_cap() -> Option<usize> {
    let n = QUERY_ROW_CAP.load(Ordering::SeqCst);
    (n > 0).then_some(n)
}

/// Records one materialized query row; `true` when the caller should stop.
#[must_use]
pub fn note_query_row() -> bool {
    let seen = QUERY_ROWS_SEEN.fetch_add(1, Ordering::SeqCst) + 1;
    query_row_cap().is_some_and(|cap| seen > cap)
}

/// Rows materialized by the current capped query (tests).
#[must_use]
pub fn query_rows_seen() -> usize {
    QUERY_ROWS_SEEN.load(Ordering::SeqCst)
}
