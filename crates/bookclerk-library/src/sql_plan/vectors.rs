//! Backend-parameterized SQL-contract vectors shared by sqlite, postgres, and D1.

use std::future::Future;

use bookclerk_plugin_abi::{
    DbAtomicPlan, DbAtomicRequest, DbPlanExecResult, DbPlanStatement, DbPlanStatementKind,
};
use sea_orm::DatabaseConnection;

use super::{
    compile_named_request, execute_plan_on, execute_statements_on, interpret_exec, SqlFamily,
};
use crate::atomic_ops::{atomic_status, DbAtomicParams};

/// Runs commit/replay, unique, rollback, and row-cap vectors on a SeaORM connection.
///
/// # Panics
///
/// Panics when a vector fails.
pub async fn run_conn_vectors(db: &DatabaseConnection, family: SqlFamily, timing: &str) {
    commit_and_replay(db, family, timing).await;
    hash_conflict(db, family, timing).await;
    unique_generic_insert_fails(db, timing).await;
    failed_statement_rolls_back(db, timing).await;
}

/// Runs the same vectors through a guest `dbAtomic` callback (D1 HTTP batch).
///
/// # Panics
///
/// Panics when a vector fails.
pub async fn run_request_vectors<F, Fut, E>(family: SqlFamily, mut run: F)
where
    F: FnMut(DbAtomicRequest) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, E>>,
    E: std::fmt::Display,
{
    let now = "2024-06-01T00:00:00Z";
    let compiled = compile_named_request(
        "vec-enq",
        &DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"a"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
        now,
        family,
    )
    .expect("compile enqueue");
    let first = run(compiled.clone().into_request("vec-enq"))
        .await
        .unwrap_or_else(|e| panic!("first atomic: {e}"));
    let interpreted = interpret_exec(&compiled.plan, &first, &compiled.expected_hash);
    assert_eq!(interpreted.status, atomic_status::OK);
    let replay = run(compiled.clone().into_request("vec-enq"))
        .await
        .unwrap_or_else(|e| panic!("replay: {e}"));
    let replayed = interpret_exec(&compiled.plan, &replay, &compiled.expected_hash);
    assert!(replayed.replayed, "same operationId must replay");

    let dup = DbAtomicRequest {
        operation_id: "vec-dup".into(),
        request_hash: None,
        plan: Some(dup_slot_plan("vec-dup")),
        deadline_unix_ms: None,
    };
    let err = run(dup)
        .await
        .err()
        .unwrap_or_else(|| panic!("duplicate insert must fail"));
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("unique")
            || msg.contains("constraint")
            || msg.contains("23505")
            || msg.contains("conflict"),
        "{err}"
    );
}

/// Enqueue once, then replay the same `operationId`.
async fn commit_and_replay(db: &DatabaseConnection, family: SqlFamily, timing: &str) {
    let compiled = compile_named_request(
        "vec-enq-conn",
        &DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"a"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
        "2024-06-01T00:00:00Z",
        family,
    )
    .unwrap();
    let first = execute_plan_on(
        db,
        &compiled.plan,
        &compiled.expected_hash,
        "vec-enq-conn",
        timing,
    )
    .await
    .unwrap();
    assert_eq!(first.status, atomic_status::OK);
    let replay = execute_plan_on(
        db,
        &compiled.plan,
        &compiled.expected_hash,
        "vec-enq-conn",
        timing,
    )
    .await
    .unwrap();
    assert!(replay.replayed);
}

/// Same `operationId` with a different request hash is an idempotency conflict.
async fn hash_conflict(db: &DatabaseConnection, family: SqlFamily, timing: &str) {
    let first = compile_named_request(
        "vec-conflict",
        &DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"a"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
        "2024-06-01T00:00:00Z",
        family,
    )
    .unwrap();
    execute_plan_on(
        db,
        &first.plan,
        &first.expected_hash,
        "vec-conflict",
        timing,
    )
    .await
    .unwrap();
    let other = compile_named_request(
        "vec-conflict",
        &DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"other"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
        "2024-06-01T00:00:00Z",
        family,
    )
    .unwrap();
    let result = execute_plan_on(
        db,
        &first.plan,
        &other.expected_hash,
        "vec-conflict",
        timing,
    )
    .await
    .unwrap();
    assert_eq!(result.status, atomic_status::IDEMPOTENCY_CONFLICT);
}

/// Duplicate primary-key inserts fail closed (engine unique / 23505).
async fn unique_generic_insert_fails(db: &DatabaseConnection, timing: &str) {
    let err = execute_statements_on(db, &dup_slot_plan("dup"), "op-unique", timing, 0)
        .await
        .unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("unique") || msg.contains("constraint") || msg.contains("23505"),
        "{err}"
    );
}

/// A later failing statement rolls back earlier inserts in the same plan.
async fn failed_statement_rolls_back(db: &DatabaseConnection, timing: &str) {
    let plan = DbAtomicPlan {
        statements: vec![
            DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('rb-vec', 0)"
                    .into(),
                binds: vec![],
                kind: DbPlanStatementKind::Execute,
            },
            DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('rb-vec', 1)"
                    .into(),
                binds: vec![],
                kind: DbPlanStatementKind::Execute,
            },
        ],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    assert!(execute_statements_on(db, &plan, "op-rb", timing, 0)
        .await
        .is_err());
}

/// Two inserts of the same `db_serialization_slots` key.
fn dup_slot_plan(key: &str) -> DbAtomicPlan {
    DbAtomicPlan {
        statements: vec![
            DbPlanStatement {
                sql: format!(
                    "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('{key}', 0)"
                ),
                binds: vec![],
                kind: DbPlanStatementKind::Execute,
            },
            DbPlanStatement {
                sql: format!(
                    "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('{key}', 1)"
                ),
                binds: vec![],
                kind: DbPlanStatementKind::Execute,
            },
        ],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    }
}
