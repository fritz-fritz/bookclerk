//! Shared SQL-plan conformance vectors (SQLite in-process).

use bookclerk_plugin_abi::{atomic_status, DbAtomicParams, DbAtomicRequest};

use super::{compile_named_request, execute_plan_on, SqlFamily};
use std::path::PathBuf;
use std::process::Command;

/// Opens an in-memory sqlite library and applies migrations.
async fn mem_db() -> sea_orm::DatabaseConnection {
    bookclerk_plugin_database_sqlite::open_memory()
        .await
        .expect("in-memory sqlite")
}

#[tokio::test]
async fn plan_commit_inserts_receipt() {
    let db = mem_db().await;
    let now = "2024-06-01T00:00:00Z";
    let req = DbAtomicRequest::named(
        "conf-enq",
        DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"a"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
    );
    let compiled = compile_named_request(&req, now, SqlFamily::Sqlite).unwrap();
    let result = execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        "conf-enq",
        "sqlite_txn",
    )
    .await
    .unwrap();
    assert_eq!(result.status, atomic_status::OK);
    assert!(!result.replayed);
    let replay = execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        "conf-enq",
        "sqlite_txn",
    )
    .await
    .unwrap();
    assert!(replay.replayed, "same operationId must replay the receipt");
    assert_eq!(replay.status, atomic_status::OK);
}

#[tokio::test]
async fn plan_hash_conflict_is_idempotency_conflict() {
    let db = mem_db().await;
    let now = "2024-06-01T00:00:00Z";
    let first = DbAtomicRequest::named(
        "conf-conflict",
        DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"a"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
    );
    let compiled = compile_named_request(&first, now, SqlFamily::Sqlite).unwrap();
    execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        "conf-conflict",
        "sqlite_txn",
    )
    .await
    .unwrap();
    let second = DbAtomicRequest::named(
        "conf-conflict",
        DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"other"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
    );
    let other = compile_named_request(&second, now, SqlFamily::Sqlite).unwrap();
    let result = execute_plan_on(
        &db,
        &compiled.plan,
        &other.expected_hash,
        "conf-conflict",
        "sqlite_txn",
    )
    .await
    .unwrap();
    assert_eq!(result.status, atomic_status::IDEMPOTENCY_CONFLICT);
}

#[tokio::test]
async fn unique_constraint_on_generic_insert_is_engine_error() {
    let db = mem_db().await;
    let plan = bookclerk_plugin_abi::DbAtomicPlan {
        statements: vec![
            bookclerk_plugin_abi::DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('dup', 0)".into(),
                binds: vec![],
                kind: bookclerk_plugin_abi::DbPlanStatementKind::Execute,
            },
            bookclerk_plugin_abi::DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('dup', 1)".into(),
                binds: vec![],
                kind: bookclerk_plugin_abi::DbPlanStatementKind::Execute,
            },
        ],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let err = execute_plan_on(&db, &plan, "hash", "op-unique", "sqlite_txn")
        .await
        .unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("unique") || msg.contains("constraint"),
        "{err}"
    );
}

#[tokio::test]
async fn failed_statement_rolls_back_earlier_inserts() {
    let db = mem_db().await;
    let plan = bookclerk_plugin_abi::DbAtomicPlan {
        statements: vec![
            bookclerk_plugin_abi::DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('rb', 0)".into(),
                binds: vec![],
                kind: bookclerk_plugin_abi::DbPlanStatementKind::Execute,
            },
            bookclerk_plugin_abi::DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('rb', 1)".into(),
                binds: vec![],
                kind: bookclerk_plugin_abi::DbPlanStatementKind::Execute,
            },
        ],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    assert!(execute_plan_on(&db, &plan, "hash", "op-rb", "sqlite_txn")
        .await
        .is_err());
    let rows: Vec<sea_orm::QueryResult> = sea_orm::ConnectionTrait::query_all_raw(
        &db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT bump FROM db_serialization_slots WHERE slot_key = 'rb'",
        ),
    )
    .await
    .unwrap();
    assert!(
        rows.is_empty(),
        "unique failure must roll back the first INSERT"
    );
}

#[tokio::test]
async fn conditional_update_zero_rows_is_ok_execute() {
    let db = mem_db().await;
    let plan = bookclerk_plugin_abi::DbAtomicPlan {
        statements: vec![
            bookclerk_plugin_abi::DbPlanStatement {
                sql: "UPDATE db_serialization_slots SET bump = 1 WHERE slot_key = 'missing'".into(),
                binds: vec![],
                kind: bookclerk_plugin_abi::DbPlanStatementKind::Execute,
            },
            bookclerk_plugin_abi::DbPlanStatement {
                sql: "SELECT 'ok' AS status".into(),
                binds: vec![],
                kind: bookclerk_plugin_abi::DbPlanStatementKind::Query,
            },
        ],
        outcome_index: 1,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let result = execute_plan_on(&db, &plan, "hash", "op-cond", "sqlite_txn")
        .await
        .unwrap();
    assert_eq!(result.status, atomic_status::OK);
}

#[tokio::test]
async fn timing_receipt_shape_is_uniform() {
    let db = mem_db().await;
    let now = "2024-06-01T00:00:00Z";
    let req = DbAtomicRequest::named(
        "conf-timing",
        DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"a"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
    );
    let compiled = compile_named_request(&req, now, SqlFamily::Sqlite).unwrap();
    let result = execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        "conf-timing",
        "sqlite_txn",
    )
    .await
    .unwrap();
    let timing = result.timing.expect("timing");
    assert!(timing.attempt_elapsed_us > 0);
    assert_eq!(timing.db_timing_source.as_deref(), Some("sqlite_txn"));
    assert!(timing.db_execution_us.is_some());
}

#[tokio::test]
async fn serialization_slot_bump_is_monotonic() {
    let db = mem_db().await;
    crate::sql_plan::lock_serialization_slot(&db, "job-queue")
        .await
        .unwrap();
    crate::sql_plan::lock_serialization_slot(&db, "job-queue")
        .await
        .unwrap();
    let rows = sea_orm::ConnectionTrait::query_all_raw(
        &db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT bump FROM db_serialization_slots WHERE slot_key = 'job-queue'",
        ),
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    let bump = rows[0].try_get_by_index::<i64>(0).unwrap();
    assert!(bump >= 2, "two locks must bump twice, got {bump}");
}

#[test]
fn postgres_renderer_uses_numbered_placeholders() {
    let now = "2024-06-01T00:00:00Z";
    let req = DbAtomicRequest::named(
        "pg-render",
        DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"pg"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
    );
    let compiled = compile_named_request(&req, now, SqlFamily::Postgres).unwrap();
    let joined = compiled
        .plan
        .statements
        .iter()
        .map(|s| s.sql.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("$1"),
        "postgres plans must use $n binds:\n{joined}"
    );
    assert!(
        !joined.contains('?'),
        "postgres plans must not leave SQLite ? placeholders:\n{joined}"
    );
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_plan_receipt_replay() {
    let url = match std::env::var("BOOKCLERK_TEST_POSTGRES_URL") {
        Ok(u) if !u.trim().is_empty() => u,
        _ => return,
    };
    let db = sea_orm::Database::connect(&url).await.expect("postgres");
    let backend = sea_orm::ConnectionTrait::get_database_backend(&db);
    for step in crate::migrations::migration_sql_postgres() {
        for stmt in step.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            sea_orm::ConnectionTrait::execute_raw(
                &db,
                sea_orm::Statement::from_string(backend, stmt.to_string()),
            )
            .await
            .unwrap_or_else(|err| panic!("postgres migration `{stmt}` failed: {err}"));
        }
    }
    let now = "2024-06-01T00:00:00Z";
    let req = DbAtomicRequest::named(
        "pg-conf-enq",
        DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: r#"{"v":1,"account":"pg"}"#.into(),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
    );
    let compiled = compile_named_request(&req, now, SqlFamily::Postgres).unwrap();
    let first = execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        "pg-conf-enq",
        "postgres_txn",
    )
    .await
    .unwrap();
    assert_eq!(first.status, atomic_status::OK);
    let replay = execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        "pg-conf-enq",
        "postgres_txn",
    )
    .await
    .unwrap();
    assert!(replay.replayed);
}

#[test]
fn architecture_lint_database_plugins() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = root.join("scripts/check-db-plugin-isolation.py");
    let status = Command::new("python3")
        .arg(&script)
        .current_dir(&root)
        .status()
        .expect("python3");
    assert!(status.success(), "check-db-plugin-isolation.py failed");
}
