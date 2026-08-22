//! Backend-parameterized SQL-contract vectors shared by sqlite, postgres, and D1.

use std::future::Future;

use bookclerk_plugin_abi::{
    DbAtomicPlan, DbAtomicRequest, DbPlanExecResult, DbPlanStatement, DbPlanStatementKind,
};
use sea_orm::DatabaseConnection;

use super::{compile_named_request, execute_statements_on, interpret_exec, SqlFamily};
use crate::atomic_ops::{atomic_status, DbAtomicParams};

/// Runs the contract suite on a SeaORM connection (sqlite / postgres).
///
/// # Panics
///
/// Panics when a vector fails.
pub async fn run_conn_vectors(db: &DatabaseConnection, family: SqlFamily, timing: &str) {
    let db = db.clone();
    let timing = timing.to_string();
    run_contract_vectors(family, move |req, cap| {
        let db = db.clone();
        let timing = timing.clone();
        async move {
            let plan = req.plan.expect("vector plan");
            execute_statements_on(&db, &plan, &req.operation_id, &timing, cap)
                .await
                .map_err(|e| e.to_string())
        }
    })
    .await;
}

/// Runs the identical contract suite through a guest `dbAtomic` callback.
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
    run_contract_vectors(family, move |req, cap| {
        let mut req = req;
        if cap > 0 {
            if let Some(plan) = req.plan.as_mut() {
                apply_readonly_caps(plan, cap);
            }
        }
        let fut = run(req);
        async move {
            let exec = fut.await.map_err(|e| e.to_string())?;
            if cap > 0 {
                for stmt in &exec.statements {
                    if stmt.rows.len() > usize::try_from(cap).unwrap_or(usize::MAX) {
                        return Err(format!(
                            "query returned {} rows; maxResultRows is {cap}",
                            stmt.rows.len()
                        ));
                    }
                }
            }
            Ok(exec)
        }
    })
    .await;
}

/// Hash-conflict, rollback, row-cap, RETURNING-cap, replay, and unique vectors.
///
/// `run` receives `(request, max_result_rows)` where `0` means unlimited.
///
/// # Panics
///
/// Panics when a vector fails.
pub async fn run_contract_vectors<F, Fut>(family: SqlFamily, mut run: F)
where
    F: FnMut(DbAtomicRequest, u32) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, String>>,
{
    commit_and_replay(&mut run, family).await;
    hash_conflict(&mut run, family).await;
    unique_generic_insert_fails(&mut run).await;
    failed_statement_rolls_back(&mut run).await;
    row_cap_select(&mut run).await;
    returning_insert_cap(&mut run).await;
    returning_update_cap(&mut run).await;
    returning_delete_cap(&mut run).await;
}

/// Enqueue once, then replay the same `operationId`.
async fn commit_and_replay<F, Fut>(run: &mut F, family: SqlFamily)
where
    F: FnMut(DbAtomicRequest, u32) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, String>>,
{
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
        "2024-06-01T00:00:00Z",
        family,
    )
    .expect("compile enqueue");
    let first = run(compiled.clone().into_request("vec-enq"), 0)
        .await
        .unwrap_or_else(|e| panic!("first atomic: {e}"));
    let interpreted = interpret_exec(&compiled.plan, &first, &compiled.expected_hash);
    assert_eq!(interpreted.status, atomic_status::OK);
    let replay = run(compiled.clone().into_request("vec-enq"), 0)
        .await
        .unwrap_or_else(|e| panic!("replay: {e}"));
    let replayed = interpret_exec(&compiled.plan, &replay, &compiled.expected_hash);
    assert!(replayed.replayed, "same operationId must replay");
}

/// Same `operationId` with a different request hash is an idempotency conflict.
async fn hash_conflict<F, Fut>(run: &mut F, family: SqlFamily)
where
    F: FnMut(DbAtomicRequest, u32) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, String>>,
{
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
    let exec = run(first.clone().into_request("vec-conflict"), 0)
        .await
        .unwrap_or_else(|e| panic!("conflict seed: {e}"));
    assert_eq!(
        interpret_exec(&first.plan, &exec, &first.expected_hash).status,
        atomic_status::OK
    );
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
    let replay = run(first.clone().into_request("vec-conflict"), 0)
        .await
        .unwrap_or_else(|e| panic!("conflict replay: {e}"));
    let result = interpret_exec(&first.plan, &replay, &other.expected_hash);
    assert_eq!(result.status, atomic_status::IDEMPOTENCY_CONFLICT);
}

/// Duplicate primary-key inserts fail closed (engine unique / 23505).
async fn unique_generic_insert_fails<F, Fut>(run: &mut F)
where
    F: FnMut(DbAtomicRequest, u32) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, String>>,
{
    let err = run(request("vec-dup", dup_slot_plan("vec-dup")), 0)
        .await
        .err()
        .unwrap_or_else(|| panic!("duplicate insert must fail"));
    let msg = err.to_lowercase();
    assert!(
        msg.contains("unique")
            || msg.contains("constraint")
            || msg.contains("23505")
            || msg.contains("conflict"),
        "{err}"
    );
}

/// A later failing statement rolls back earlier inserts in the same plan.
async fn failed_statement_rolls_back<F, Fut>(run: &mut F)
where
    F: FnMut(DbAtomicRequest, u32) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, String>>,
{
    assert!(run(request("vec-rb", dup_slot_plan("rb-vec")), 0)
        .await
        .is_err());
    let check = run(
        request(
            "vec-rb-check",
            query_plan("SELECT slot_key FROM db_serialization_slots WHERE slot_key = 'rb-vec'"),
        ),
        0,
    )
    .await
    .unwrap_or_else(|e| panic!("rollback check: {e}"));
    assert!(
        check.statements[0].rows.is_empty(),
        "failed plan must not leave the first insert: {:?}",
        check.statements[0].rows
    );
}

/// Read-only SELECT stops after `maxResultRows`.
async fn row_cap_select<F, Fut>(run: &mut F)
where
    F: FnMut(DbAtomicRequest, u32) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, String>>,
{
    run(
        request(
            "vec-cap-setup",
            exec_plan(&[
                "CREATE TABLE IF NOT EXISTS vec_rowcap (x INTEGER)",
                "DELETE FROM vec_rowcap",
            ]),
        ),
        0,
    )
    .await
    .unwrap_or_else(|e| panic!("rowcap setup: {e}"));
    let mut inserts = vec!["CREATE TABLE IF NOT EXISTS vec_rowcap (x INTEGER)".into()];
    for i in 0..20 {
        inserts.push(format!("INSERT INTO vec_rowcap (x) VALUES ({i})"));
    }
    run(request("vec-cap-ins", exec_plan_owned(inserts)), 0)
        .await
        .unwrap_or_else(|e| panic!("rowcap insert: {e}"));
    let err = run(
        request("vec-cap-sel", query_plan("SELECT x FROM vec_rowcap")),
        5,
    )
    .await
    .expect_err("over-cap SELECT must fail");
    assert!(
        err.to_lowercase().contains("maxresultrows"),
        "row cap must fail closed: {err}"
    );
}

/// `INSERT … RETURNING` is not rewritten as a subquery and still honors the cap.
async fn returning_insert_cap<F, Fut>(run: &mut F)
where
    F: FnMut(DbAtomicRequest, u32) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, String>>,
{
    run(
        request(
            "vec-ret-ins-setup",
            exec_plan(&[
                "CREATE TABLE IF NOT EXISTS vec_ret_ins (id INTEGER PRIMARY KEY)",
                "DELETE FROM vec_ret_ins",
            ]),
        ),
        0,
    )
    .await
    .unwrap_or_else(|e| panic!("returning insert setup: {e}"));
    let sql = "INSERT INTO vec_ret_ins (id) \
         SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 \
         UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9 UNION ALL SELECT 10 \
         RETURNING id";
    let err = run(request("vec-ret-ins", query_plan(sql)), 5)
        .await
        .expect_err("capped INSERT RETURNING must fail");
    assert!(
        !err.to_lowercase().contains("syntax"),
        "INSERT RETURNING must not be wrapped as a subquery: {err}"
    );
    assert!(
        err.to_lowercase().contains("maxresultrows"),
        "INSERT RETURNING cap must fail closed: {err}"
    );
}

/// `UPDATE … RETURNING` honors the row cap without a subquery rewrite.
async fn returning_update_cap<F, Fut>(run: &mut F)
where
    F: FnMut(DbAtomicRequest, u32) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, String>>,
{
    let mut stmts = vec![
        "CREATE TABLE IF NOT EXISTS vec_ret_upd (id INTEGER PRIMARY KEY)".into(),
        "DELETE FROM vec_ret_upd".into(),
    ];
    for i in 1..=10 {
        stmts.push(format!("INSERT INTO vec_ret_upd (id) VALUES ({i})"));
    }
    run(request("vec-ret-upd-setup", exec_plan_owned(stmts)), 0)
        .await
        .unwrap_or_else(|e| panic!("returning update setup: {e}"));
    let err = run(
        request(
            "vec-ret-upd",
            query_plan("UPDATE vec_ret_upd SET id = id RETURNING id"),
        ),
        5,
    )
    .await
    .expect_err("capped UPDATE RETURNING must fail");
    assert!(
        !err.to_lowercase().contains("syntax"),
        "UPDATE RETURNING must not be wrapped as a subquery: {err}"
    );
    assert!(
        err.to_lowercase().contains("maxresultrows"),
        "UPDATE RETURNING cap must fail closed: {err}"
    );
}

/// `DELETE … RETURNING` honors the row cap without a subquery rewrite.
async fn returning_delete_cap<F, Fut>(run: &mut F)
where
    F: FnMut(DbAtomicRequest, u32) -> Fut,
    Fut: Future<Output = Result<DbPlanExecResult, String>>,
{
    let mut stmts = vec![
        "CREATE TABLE IF NOT EXISTS vec_ret_del (id INTEGER PRIMARY KEY)".into(),
        "DELETE FROM vec_ret_del".into(),
    ];
    for i in 1..=10 {
        stmts.push(format!("INSERT INTO vec_ret_del (id) VALUES ({i})"));
    }
    run(request("vec-ret-del-setup", exec_plan_owned(stmts)), 0)
        .await
        .unwrap_or_else(|e| panic!("returning delete setup: {e}"));
    let err = run(
        request(
            "vec-ret-del",
            query_plan("DELETE FROM vec_ret_del RETURNING id"),
        ),
        5,
    )
    .await
    .expect_err("capped DELETE RETURNING must fail");
    assert!(
        !err.to_lowercase().contains("syntax"),
        "DELETE RETURNING must not be wrapped as a subquery: {err}"
    );
    assert!(
        err.to_lowercase().contains("maxresultrows"),
        "DELETE RETURNING cap must fail closed: {err}"
    );
}

/// Wraps read-only SELECT statements with `LIMIT cap+1`.
fn apply_readonly_caps(plan: &mut DbAtomicPlan, cap: u32) {
    for stmt in &mut plan.statements {
        if stmt.kind == DbPlanStatementKind::Query {
            stmt.sql = bookclerk_db_exec::cap_query_sql(&stmt.sql, cap);
        }
    }
}

/// Envelope for a vector plan with no request hash.
fn request(operation_id: &str, plan: DbAtomicPlan) -> DbAtomicRequest {
    DbAtomicRequest {
        operation_id: operation_id.into(),
        request_hash: None,
        plan: Some(plan),
        deadline_unix_ms: None,
    }
}

/// Single Query statement plan.
fn query_plan(sql: &str) -> DbAtomicPlan {
    DbAtomicPlan {
        statements: vec![DbPlanStatement {
            sql: sql.into(),
            binds: vec![],
            kind: DbPlanStatementKind::Query,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    }
}

/// Execute-only plan from borrowed SQL strings.
fn exec_plan(sqls: &[&str]) -> DbAtomicPlan {
    exec_plan_owned(sqls.iter().map(|s| (*s).to_string()).collect())
}

/// Execute-only plan from owned SQL strings.
fn exec_plan_owned(sqls: Vec<String>) -> DbAtomicPlan {
    DbAtomicPlan {
        statements: sqls
            .into_iter()
            .map(|sql| DbPlanStatement {
                sql,
                binds: vec![],
                kind: DbPlanStatementKind::Execute,
            })
            .collect(),
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    }
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
