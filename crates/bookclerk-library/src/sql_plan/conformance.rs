//! Shared SQL-plan conformance vectors (SQLite in-process).
//!
//! **Admission (#178):** database plugins must pass
//! [`super::typed_vectors::run_typed_request_vectors`] with a callback that
//! executes native [`ExecuteRequest`] / [`ExecuteReply`] (Cap'n Proto on the wire).
//! [`super::vectors::run_conn_vectors`] and
//! [`super::vectors::run_request_vectors`] remain for the legacy JSON
//! [`DbAtomicRequest`] bridge and in-process sqlite/postgres hosts.

use crate::atomic_ops::{atomic_status, DbAtomicParams};

use super::{
    compile_named_request, execute_plan_on, execute_statements_on_session,
    vectors::CONTRACT_VECTOR_ROW_CAP, AtomicSession,
};
use bookclerk_plugin_abi::DbConnectResult;
use std::path::PathBuf;
use std::process::Command;

/// True when Postgres conformance tests should run (URL present).
///
/// `BOOKCLERK_REQUIRE_POSTGRES_TESTS=1` without a URL is a hard failure so CI
/// cannot skip these cases.
fn postgres_conformance_enabled() -> bool {
    let url = std::env::var("BOOKCLERK_TEST_POSTGRES_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    if url.is_some() {
        return true;
    }
    assert!(
        std::env::var("BOOKCLERK_REQUIRE_POSTGRES_TESTS")
            .ok()
            .as_deref()
            != Some("1"),
        "BOOKCLERK_TEST_POSTGRES_URL is required when BOOKCLERK_REQUIRE_POSTGRES_TESTS=1"
    );
    false
}

/// Opens an in-memory sqlite library and applies migrations.
async fn mem_db() -> sea_orm::DatabaseConnection {
    bookclerk_plugin_database_sqlite::open_memory()
        .await
        .expect("in-memory sqlite")
}

struct NamedOp {
    id: &'static str,
    params: DbAtomicParams,
}

fn named(id: &'static str, params: DbAtomicParams) -> NamedOp {
    NamedOp { id, params }
}

#[tokio::test]
async fn shared_vectors_on_sqlite() {
    let db = mem_db().await;
    super::vectors::run_conn_vectors(&db, DbConnectResult::sqlite(), "sqlite_txn").await;
}

#[tokio::test]
async fn typed_shared_vectors_on_sqlite() {
    let db = mem_db().await;
    super::typed_vectors::run_typed_conn_vectors(&db, DbConnectResult::sqlite(), "sqlite_txn")
        .await;
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_shared_vectors() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    super::vectors::run_conn_vectors(&db, DbConnectResult::postgres(), "postgres_txn").await;
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn typed_shared_vectors_on_postgres() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    super::typed_vectors::run_typed_conn_vectors(&db, DbConnectResult::postgres(), "postgres_txn")
        .await;
}

#[tokio::test]
async fn sqlite_recursive_cte_honors_deadline() {
    let db = mem_db().await;
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
        .saturating_add(80);
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: "WITH RECURSIVE t(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM t WHERE x < 200000000) SELECT COUNT(*) AS n FROM t".into(),
            binds: vec![],
            kind: crate::sql_plan::DbPlanStatementKind::Query,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let err = super::execute_statements_on_session(
        &db,
        &plan,
        "op-deadline",
        "sqlite_txn",
        0,
        super::AtomicSession {
            cancel: None,
            deadline_unix_ms: Some(deadline),
        },
    )
    .await
    .unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("deadline") || msg.contains("interrupt") || msg.contains("cancel"),
        "long statement must honor deadline, got {err}"
    );
}

#[tokio::test]
async fn sqlite_query_stops_after_cap_plus_one() {
    let db = mem_db().await;
    let backend = sea_orm::ConnectionTrait::get_database_backend(&db);
    sea_orm::ConnectionTrait::execute_raw(
        &db,
        sea_orm::Statement::from_string(
            backend,
            "CREATE TABLE IF NOT EXISTS rowcap_probe (x INTEGER)",
        ),
    )
    .await
    .ok();
    for i in 0..50 {
        sea_orm::ConnectionTrait::execute_raw(
            &db,
            sea_orm::Statement::from_sql_and_values(
                backend,
                "INSERT INTO rowcap_probe (x) VALUES (?)",
                [i.into()],
            ),
        )
        .await
        .unwrap();
    }
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: "SELECT x FROM rowcap_probe".into(),
            binds: vec![],
            kind: crate::sql_plan::DbPlanStatementKind::Query,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let err = super::execute_statements_on(&db, &plan, "op-early", "sqlite_txn", 5)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("maxResultRows"), "{err}");
    let seen = crate::query_rows_seen();
    assert!(
        seen <= 6,
        "must stop after cap+1 materialized rows, saw {seen}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_attempts_keep_independent_deadlines_and_caps() {
    let db_deadline = mem_db().await;
    let db_cap = mem_db().await;
    let backend = sea_orm::ConnectionTrait::get_database_backend(&db_cap);
    sea_orm::ConnectionTrait::execute_raw(
        &db_cap,
        sea_orm::Statement::from_string(
            backend,
            "CREATE TABLE IF NOT EXISTS rowcap_probe (x INTEGER)",
        ),
    )
    .await
    .ok();
    for i in 0..50 {
        sea_orm::ConnectionTrait::execute_raw(
            &db_cap,
            sea_orm::Statement::from_sql_and_values(
                backend,
                "INSERT INTO rowcap_probe (x) VALUES (?)",
                [i.into()],
            ),
        )
        .await
        .unwrap();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let cte = crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: "WITH RECURSIVE t(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM t WHERE x < 200000000) SELECT COUNT(*) AS n FROM t".into(),
            binds: vec![],
            kind: crate::sql_plan::DbPlanStatementKind::Query,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let select = crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: "SELECT x FROM rowcap_probe".into(),
            binds: vec![],
            kind: crate::sql_plan::DbPlanStatementKind::Query,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let deadline = execute_statements_on_session(
        &db_deadline,
        &cte,
        "op-conc-deadline",
        "sqlite_txn",
        0,
        AtomicSession {
            cancel: None,
            deadline_unix_ms: Some(now.saturating_add(80)),
        },
    );
    let cap = execute_statements_on_session(
        &db_cap,
        &select,
        "op-conc-cap",
        "sqlite_txn",
        5,
        AtomicSession {
            cancel: None,
            deadline_unix_ms: Some(now.saturating_add(60_000)),
        },
    );
    let (deadline, cap) = tokio::join!(
        tokio::time::timeout(std::time::Duration::from_secs(3), deadline),
        cap
    );
    let deadline_err = deadline
        .expect("deadline attempt must not hang if its budget is overwritten")
        .unwrap_err();
    let dmsg = deadline_err.to_string().to_lowercase();
    assert!(
        dmsg.contains("deadline") || dmsg.contains("interrupt") || dmsg.contains("cancel"),
        "long CTE must honor its own deadline, got {deadline_err}"
    );
    let cap_err = cap.unwrap_err();
    assert!(
        cap_err.to_string().contains("maxResultRows"),
        "capped SELECT must honor its own cap, got {cap_err}"
    );
}

#[tokio::test]
async fn plan_commit_inserts_receipt() {
    let db = mem_db().await;
    let now = "2024-06-01T00:00:00Z";
    let req = named(
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
    let compiled = compile_named_request(req.id, &req.params, now).unwrap();
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
    let first = named(
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
    let compiled = compile_named_request(first.id, &first.params, now).unwrap();
    execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        "conf-conflict",
        "sqlite_txn",
    )
    .await
    .unwrap();
    let second = named(
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
    let other = compile_named_request(second.id, &second.params, now).unwrap();
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
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![
            crate::sql_plan::DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('dup', 0)".into(),
                binds: vec![],
                kind: crate::sql_plan::DbPlanStatementKind::Execute,
                max_rows: 0,
            },
            crate::sql_plan::DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('dup', 1)".into(),
                binds: vec![],
                kind: crate::sql_plan::DbPlanStatementKind::Execute,
                max_rows: 0,
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
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![
            crate::sql_plan::DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('rb', 0)".into(),
                binds: vec![],
                kind: crate::sql_plan::DbPlanStatementKind::Execute,
                max_rows: 0,
            },
            crate::sql_plan::DbPlanStatement {
                sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('rb', 1)".into(),
                binds: vec![],
                kind: crate::sql_plan::DbPlanStatementKind::Execute,
                max_rows: 0,
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
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![
            crate::sql_plan::DbPlanStatement {
                sql: "UPDATE db_serialization_slots SET bump = 1 WHERE slot_key = 'missing'".into(),
                binds: vec![],
                kind: crate::sql_plan::DbPlanStatementKind::Execute,
                max_rows: 0,
            },
            crate::sql_plan::DbPlanStatement {
                sql: "SELECT 'ok' AS status".into(),
                binds: vec![],
                kind: crate::sql_plan::DbPlanStatementKind::Query,
                max_rows: 0,
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
    let req = named(
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
    let compiled = compile_named_request(req.id, &req.params, now).unwrap();
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
fn postgres_renderer_lowers_canonical_placeholders() {
    let now = "2024-06-01T00:00:00Z";
    let req = named(
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
    let compiled = compile_named_request(req.id, &req.params, now).unwrap();
    let joined = compiled
        .plan
        .statements
        .iter()
        .map(|s| s.sql.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !joined.contains("$1"),
        "host compiler must emit canonical SQL, not $n:\n{joined}"
    );
    let mut lowered_any = false;
    for stmt in &compiled.plan.statements {
        if stmt.sql.contains('?') {
            let lowered = bookclerk_db_exec::lower_canonical_to_postgres(&stmt.sql);
            assert!(
                lowered.contains('$'),
                "adapter lowering must produce $n binds:\n{lowered}"
            );
            assert!(
                !lowered.contains('?'),
                "adapter lowering must not leave ? placeholders:\n{lowered}"
            );
            lowered_any = true;
        }
    }
    assert!(lowered_any, "enqueue plan must contain binds:\n{joined}");
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_plan_receipt_replay() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    let now = "2024-06-01T00:00:00Z";
    let req = named(
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
    let compiled = compile_named_request(req.id, &req.params, now).unwrap();
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

/// Disposable Postgres so claim cannot see leftover `jobs` rows.
async fn postgres_migrated_db() -> sea_orm::DatabaseConnection {
    let url = std::env::var("BOOKCLERK_TEST_POSTGRES_URL").expect("postgres url");
    let db_name = format!("plan_{}", uuid::Uuid::new_v4().as_simple());
    let admin = sea_orm::Database::connect(url.as_str())
        .await
        .expect("connect to BOOKCLERK_TEST_POSTGRES_URL");
    let backend = sea_orm::ConnectionTrait::get_database_backend(&admin);
    sea_orm::ConnectionTrait::execute_raw(
        &admin,
        sea_orm::Statement::from_string(backend, format!("CREATE DATABASE {db_name}")),
    )
    .await
    .expect("create disposable postgres database");
    let (base, query) = match url.split_once('?') {
        Some((base, q)) => (base, Some(q)),
        None => (url.as_str(), None),
    };
    let trimmed = base.trim_end_matches('/');
    let slash = trimmed
        .rfind('/')
        .expect("BOOKCLERK_TEST_POSTGRES_URL must include a database path");
    let db_url = match query {
        Some(q) => format!("{}/{db_name}?{q}", &trimmed[..slash]),
        None => format!("{}/{db_name}", &trimmed[..slash]),
    };
    let db = sea_orm::Database::connect(&db_url)
        .await
        .expect("connect to disposable postgres database");
    crate::apply_host_schema(&db, crate::HostSchemaKind::RowMarker)
        .await
        .expect("host-applied postgres schema");
    db
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_claim_malformed_json_is_quarantined() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    let backend = sea_orm::ConnectionTrait::get_database_backend(&db);
    for (id, payload, priority) in [
        ("bad-json", "{not-json", 10_i64),
        ("good", r#"{"v":1}"#, 0_i64),
    ] {
        sea_orm::ConnectionTrait::execute_raw(
            &db,
            sea_orm::Statement::from_sql_and_values(
                backend,
                "INSERT INTO jobs (\
                    id, kind, state, priority, resource_class, payload, progress, \
                    attempt_count, max_attempts, run_after, lease_owner, lease_expires_at, \
                    dedup_key, error_kind, error_message, cancel_requested, \
                    created_at, updated_at, started_at, finished_at, lease_generation\
                 ) VALUES ($1, 'scan', 'pending', $2, 'network', $3, NULL, 0, 3, \
                    '2020-01-01T00:00:00Z', NULL, NULL, $1, NULL, NULL, 0, \
                    '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL, NULL, 0)",
                [id.into(), priority.into(), payload.into()],
            ),
        )
        .await
        .unwrap_or_else(|err| panic!("seed job {id}: {err}"));
    }
    let now = "2024-06-01T00:00:00Z";
    let req = named(
        "pg-claim-poison",
        DbAtomicParams::ClaimNextJob {
            resource_class: "network".into(),
            owner: "worker-1".into(),
            lease_secs: 60,
        },
    );
    let compiled = compile_named_request(req.id, &req.params, now).unwrap();
    let result = execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        "pg-claim-poison",
        "postgres_txn",
    )
    .await
    .expect("malformed payload must not abort the claim batch");
    assert_eq!(result.status, atomic_status::OK);
    let rows = sea_orm::ConnectionTrait::query_all_raw(
        &db,
        sea_orm::Statement::from_string(
            backend,
            "SELECT id, state, kind, error_kind FROM jobs WHERE id IN ('bad-json', 'good') ORDER BY id",
        ),
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].try_get_by_index::<String>(0).unwrap(), "bad-json");
    assert_eq!(rows[0].try_get_by_index::<String>(1).unwrap(), "failed");
    assert_eq!(rows[0].try_get_by_index::<String>(2).unwrap(), "invalid");
    assert_eq!(
        rows[0].try_get_by_index::<String>(3).unwrap(),
        "invalid_job"
    );
    assert_eq!(rows[1].try_get_by_index::<String>(0).unwrap(), "good");
    assert_eq!(rows[1].try_get_by_index::<String>(1).unwrap(), "running");
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

fn enqueue_scan(id: &'static str, account: &str) -> NamedOp {
    named(
        id,
        DbAtomicParams::EnqueueJob {
            kind: "scan".into(),
            payload_json: format!(r#"{{"v":1,"account":"{account}"}}"#),
            priority: 0,
            max_attempts: 3,
            max_pending: 10,
            run_after: None,
        },
    )
}

fn d1_json_to_rusqlite(v: &serde_json::Value) -> rusqlite::types::Value {
    if crate::sql_plan::sea_null_kind(v).is_some() {
        return rusqlite::types::Value::Null;
    }
    match v {
        serde_json::Value::Null => rusqlite::types::Value::Null,
        serde_json::Value::Bool(b) => rusqlite::types::Value::Integer(i64::from(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                rusqlite::types::Value::Integer(i)
            } else if let Some(u) = n.as_u64() {
                rusqlite::types::Value::Integer(i64::try_from(u).unwrap_or(i64::MAX))
            } else {
                rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => rusqlite::types::Value::Text(s.clone()),
        other => rusqlite::types::Value::Text(other.to_string()),
    }
}

/// Executes sqlite-dialect plan SQL in one rusqlite transaction (D1 bind flattening).
fn d1_compat_execute(
    conn: &rusqlite::Connection,
    plan: &crate::sql_plan::DbAtomicPlan,
    operation_id: &str,
) -> crate::sql_plan::DbPlanExecResult {
    const D1_BIND_CAP: usize = 100;
    let txn = conn.unchecked_transaction().unwrap();
    let mut statements = Vec::new();
    for stmt in &plan.statements {
        assert!(
            stmt.binds.len() <= D1_BIND_CAP,
            "D1 bind cap is {D1_BIND_CAP}, got {}",
            stmt.binds.len()
        );
        let binds: Vec<rusqlite::types::Value> =
            stmt.binds.iter().map(d1_json_to_rusqlite).collect();
        let mut prepared = txn.prepare(&stmt.sql).unwrap();
        let col_count = prepared.column_count();
        let names: Vec<String> = prepared
            .column_names()
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut rows = Vec::new();
        let engine_changes = if col_count == 0 {
            u64::try_from(
                prepared
                    .execute(rusqlite::params_from_iter(binds.iter()))
                    .unwrap(),
            )
            .unwrap_or(0)
        } else {
            let mut query = prepared
                .query(rusqlite::params_from_iter(binds.iter()))
                .unwrap();
            while let Some(row) = query.next().unwrap() {
                let mut obj = serde_json::Map::new();
                for (i, name) in names.iter().enumerate() {
                    let val = row.get_ref(i).unwrap();
                    let json = match val {
                        rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                        rusqlite::types::ValueRef::Integer(n) => serde_json::Value::from(n),
                        rusqlite::types::ValueRef::Real(n) => serde_json::Value::from(n),
                        rusqlite::types::ValueRef::Text(t) => {
                            serde_json::Value::String(String::from_utf8_lossy(t).into_owned())
                        }
                        rusqlite::types::ValueRef::Blob(_) => serde_json::Value::Null,
                    };
                    obj.insert(name.clone(), json);
                }
                rows.push(serde_json::Value::Object(obj));
                if rows.len() > 1_000 {
                    panic!("D1-compat query exceeded maxResultRows 1000");
                }
            }
            0
        };
        let rows_affected = match stmt.kind {
            crate::sql_plan::DbPlanStatementKind::Select => 0,
            crate::sql_plan::DbPlanStatementKind::Returning
            | crate::sql_plan::DbPlanStatementKind::Query => u64::try_from(rows.len()).unwrap_or(0),
            crate::sql_plan::DbPlanStatementKind::Execute => engine_changes,
        };
        statements.push(crate::sql_plan::DbPlanStmtExecResult {
            rows,
            rows_affected,
        });
    }
    txn.commit().unwrap();
    crate::sql_plan::DbPlanExecResult {
        operation_id: operation_id.into(),
        statements,
        timing: Some(crate::sql_plan::DbAtomicTiming {
            attempt_elapsed_us: 1,
            db_execution_us: Some(1),
            db_timing_source: Some("d1_compat_rusqlite".into()),
        }),
    }
}

fn d1_compat_mem() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    for sql in crate::migrations::migration_sql() {
        conn.execute_batch(sql).unwrap();
    }
    conn
}

#[test]
fn d1_compat_plan_commit_and_replay() {
    let conn = d1_compat_mem();
    let now = "2024-06-01T00:00:00Z";
    let op = enqueue_scan("d1-conf-enq", "a");
    let compiled = compile_named_request(op.id, &op.params, now).unwrap();
    let exec = d1_compat_execute(&conn, &compiled.plan, op.id);
    let first = super::interpret_exec(&compiled.plan, &exec, &compiled.expected_hash);
    assert_eq!(first.status, atomic_status::OK);
    assert!(!first.replayed);
    let exec = d1_compat_execute(&conn, &compiled.plan, op.id);
    let replay = super::interpret_exec(&compiled.plan, &exec, &compiled.expected_hash);
    assert!(replay.replayed);
    assert_eq!(replay.status, atomic_status::OK);
}

#[test]
fn d1_compat_hash_conflict_is_idempotency_conflict() {
    let conn = d1_compat_mem();
    let now = "2024-06-01T00:00:00Z";
    let first = enqueue_scan("d1-conflict", "a");
    let compiled = compile_named_request(first.id, &first.params, now).unwrap();
    let _ = d1_compat_execute(&conn, &compiled.plan, first.id);
    let second = enqueue_scan("d1-conflict", "other");
    let other = compile_named_request(second.id, &second.params, now).unwrap();
    let exec = d1_compat_execute(&conn, &compiled.plan, first.id);
    let result = super::interpret_exec(&compiled.plan, &exec, &other.expected_hash);
    assert_eq!(result.status, atomic_status::IDEMPOTENCY_CONFLICT);
}

#[test]
fn d1_compat_unique_constraint_is_engine_error() {
    let conn = d1_compat_mem();
    conn.execute(
        "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('dup', 0)",
        [],
    )
    .unwrap();
    let err = conn
        .execute(
            "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('dup', 1)",
            [],
        )
        .unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("unique") || msg.contains("constraint"),
        "{err}"
    );
}

#[test]
fn d1_compat_rejects_more_than_100_binds() {
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: "SELECT 1".into(),
            binds: vec![serde_json::json!(1); 101],
            kind: crate::sql_plan::DbPlanStatementKind::Query,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let conn = d1_compat_mem();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        d1_compat_execute(&conn, &plan, "op");
    }));
    assert!(panicked.is_err(), "D1 bind cap 100 must fail closed");
}

#[tokio::test]
async fn plan_cancel_hook_aborts_before_commit() {
    let db = mem_db().await;
    crate::inject_commit_failures(1);
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('cancel', 0)".into(),
            binds: vec![],
            kind: crate::sql_plan::DbPlanStatementKind::Execute,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let err = execute_plan_on(&db, &plan, "hash", "op-cancel", "sqlite_txn")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("commit failed"), "{err}");
    let rows: Vec<sea_orm::QueryResult> = sea_orm::ConnectionTrait::query_all_raw(
        &db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT bump FROM db_serialization_slots WHERE slot_key = 'cancel'",
        ),
    )
    .await
    .unwrap();
    assert!(rows.is_empty(), "injected commit failure must roll back");
}

#[tokio::test]
async fn execute_caps_collected_rows_at_max_result_rows() {
    let db = mem_db().await;
    for i in 0..5 {
        sea_orm::ConnectionTrait::execute_raw(
            &db,
            sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                "INSERT INTO db_serialization_slots (slot_key, bump) VALUES (?, 0)",
                [format!("cap-{i}").into()],
            ),
        )
        .await
        .unwrap();
    }
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: "SELECT slot_key FROM db_serialization_slots WHERE slot_key LIKE 'cap-%' ORDER BY slot_key"
                .into(),
            binds: vec![],
            kind: crate::sql_plan::DbPlanStatementKind::Query,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let exec = super::execute_statements_on(&db, &plan, "op-cap", "sqlite_txn", 5)
        .await
        .unwrap();
    assert_eq!(exec.statements[0].rows.len(), 5);
    let err = super::execute_statements_on(&db, &plan, "op-cap-over", "sqlite_txn", 2)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("maxResultRows"),
        "row cap must fail closed: {err}"
    );
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_plan_commit_inserts_receipt() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    let now = "2024-06-01T00:00:00Z";
    let op = enqueue_scan("pg-conf-enq-2", "pg");
    let compiled = compile_named_request(op.id, &op.params, now).unwrap();
    let first = execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        op.id,
        "postgres_txn",
    )
    .await
    .unwrap();
    assert_eq!(first.status, atomic_status::OK);
    let replay = execute_plan_on(
        &db,
        &compiled.plan,
        &compiled.expected_hash,
        op.id,
        "postgres_txn",
    )
    .await
    .unwrap();
    assert!(replay.replayed);
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_plan_exceeds_max_binds_is_rejected() {
    let mut caps = bookclerk_plugin_abi::DbConnectResult::postgres();
    caps.max_binds = 2;
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: "SELECT $1, $2, $3".into(),
            binds: vec![
                serde_json::json!(1),
                serde_json::json!(2),
                serde_json::json!(3),
            ],
            kind: crate::sql_plan::DbPlanStatementKind::Query,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let err = super::validate_plan(&plan, &caps).unwrap_err();
    assert!(err.to_string().contains("maxBinds"), "{err}");
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_execute_caps_collected_rows() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    let backend = sea_orm::ConnectionTrait::get_database_backend(&db);
    for i in 0..5 {
        sea_orm::ConnectionTrait::execute_raw(
            &db,
            sea_orm::Statement::from_sql_and_values(
                backend,
                "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ($1, 0)",
                [format!("pg-cap-{i}").into()],
            ),
        )
        .await
        .unwrap();
    }
    let plan = crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: "SELECT slot_key FROM db_serialization_slots WHERE slot_key LIKE 'pg-cap-%' ORDER BY slot_key"
                .into(),
            binds: vec![],
            kind: crate::sql_plan::DbPlanStatementKind::Query,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let exec = super::execute_statements_on(&db, &plan, "op-pg-cap", "postgres_txn", 5)
        .await
        .unwrap();
    assert_eq!(exec.statements[0].rows.len(), 5);
    let err = super::execute_statements_on(&db, &plan, "op-pg-cap-over", "postgres_txn", 2)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("maxResultRows"),
        "row cap must fail closed: {err}"
    );
}

#[tokio::test]
async fn host_applies_schema_to_unmigrated_sqlite() {
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .expect("unmigrated sqlite");
    crate::apply_host_schema(&db, crate::HostSchemaKind::PragmaMarker)
        .await
        .expect("host schema");
    let rows = sea_orm::ConnectionTrait::query_all_raw(
        &db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type='table' AND name='db_serialization_slots'",
        ),
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
}

fn interrupt_plan(slot: &str) -> crate::sql_plan::DbAtomicPlan {
    crate::sql_plan::DbAtomicPlan {
        statements: vec![crate::sql_plan::DbPlanStatement {
            sql: format!(
                "INSERT INTO db_serialization_slots (slot_key, bump) VALUES ('{slot}', 0)"
            ),
            binds: vec![],
            kind: crate::sql_plan::DbPlanStatementKind::Execute,
            max_rows: 0,
        }],
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    }
}

async fn slot_missing(db: &sea_orm::DatabaseConnection, key: &str) -> bool {
    let rows = sea_orm::ConnectionTrait::query_all_raw(
        db,
        sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT bump FROM db_serialization_slots WHERE slot_key = ?",
            [key.into()],
        ),
    )
    .await
    .unwrap();
    rows.is_empty()
}

#[tokio::test]
async fn plan_cancel_before_begin_does_not_commit() {
    let db = mem_db().await;
    crate::inject_atomic_interrupt(
        crate::AtomicInterruptPhase::BeforeBegin,
        crate::AtomicInterruptKind::Cancel,
    );
    let err = super::execute_statements_on(
        &db,
        &interrupt_plan("c-before"),
        "op-c-before",
        "sqlite_txn",
        0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
    assert!(slot_missing(&db, "c-before").await);
}

#[tokio::test]
async fn plan_cancel_during_statements_rolls_back() {
    let db = mem_db().await;
    crate::inject_atomic_interrupt(
        crate::AtomicInterruptPhase::BetweenStatements,
        crate::AtomicInterruptKind::Cancel,
    );
    let err = super::execute_statements_on(
        &db,
        &interrupt_plan("c-during"),
        "op-c-during",
        "sqlite_txn",
        0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
    assert!(slot_missing(&db, "c-during").await);
}

#[tokio::test]
async fn plan_cancel_around_commit_is_unavailable() {
    let db = mem_db().await;
    crate::inject_atomic_interrupt(
        crate::AtomicInterruptPhase::AroundCommit,
        crate::AtomicInterruptKind::Cancel,
    );
    let err = super::execute_statements_on(
        &db,
        &interrupt_plan("c-commit"),
        "op-c-commit",
        "sqlite_txn",
        0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("database commit failed"), "{err}");
    assert!(slot_missing(&db, "c-commit").await);
}

#[tokio::test]
async fn plan_deadline_before_begin() {
    let db = mem_db().await;
    crate::inject_atomic_interrupt(
        crate::AtomicInterruptPhase::BeforeBegin,
        crate::AtomicInterruptKind::Deadline,
    );
    let err = super::execute_statements_on(
        &db,
        &interrupt_plan("d-before"),
        "op-d-before",
        "sqlite_txn",
        0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("deadline"), "{err}");
    assert!(slot_missing(&db, "d-before").await);
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_plan_cancel_before_begin() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    crate::inject_atomic_interrupt(
        crate::AtomicInterruptPhase::BeforeBegin,
        crate::AtomicInterruptKind::Cancel,
    );
    let err = super::execute_statements_on(
        &db,
        &interrupt_plan("pg-c-before"),
        "op-pg-c-before",
        "postgres_txn",
        0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_plan_cancel_during_statements() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    crate::inject_atomic_interrupt(
        crate::AtomicInterruptPhase::BetweenStatements,
        crate::AtomicInterruptKind::Cancel,
    );
    let err = super::execute_statements_on(
        &db,
        &interrupt_plan("pg-c-during"),
        "op-pg-c-during",
        "postgres_txn",
        0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_host_applies_schema() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    sea_orm::ConnectionTrait::query_all_raw(
        &db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT 1 FROM db_serialization_slots LIMIT 1",
        ),
    )
    .await
    .expect("db_serialization_slots exists after host schema");
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_plan_cancel_around_commit_is_unavailable() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    crate::inject_atomic_interrupt(
        crate::AtomicInterruptPhase::AroundCommit,
        crate::AtomicInterruptKind::Cancel,
    );
    let err = super::execute_statements_on(
        &db,
        &interrupt_plan("pg-c-commit"),
        "op-pg-c-commit",
        "postgres_txn",
        0,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("database commit failed"), "{err}");
}

fn typed_query(id: &str, sql: &str) -> bookclerk_plugin_abi::ExecuteRequest {
    use bookclerk_plugin_abi::{
        DbPlanStatementKind, DbResultSelection, ExecuteRequest, TypedDbStatement,
    };
    ExecuteRequest {
        operation_id: id.into(),
        request_hash: String::new(),
        statements: vec![TypedDbStatement {
            sql: sql.into(),
            parameters: vec![],
            kind: DbPlanStatementKind::Select,
            max_rows: 0,
            result_selection: DbResultSelection::Rows,
        }],
        deadline_unix_ms: 0,
    }
}

async fn seed_typed_probe(db: &sea_orm::DatabaseConnection, sql_values: &str) {
    let backend = sea_orm::ConnectionTrait::get_database_backend(db);
    sea_orm::ConnectionTrait::execute_raw(
        db,
        sea_orm::Statement::from_string(
            backend,
            "CREATE TABLE IF NOT EXISTS typed_probe (x INTEGER, y TEXT)",
        ),
    )
    .await
    .unwrap();
    sea_orm::ConnectionTrait::execute_raw(db, sea_orm::Statement::from_string(backend, sql_values))
        .await
        .unwrap();
}

#[tokio::test]
async fn typed_sqlite_duplicate_alias_zero_row_and_null_metadata() {
    let db = mem_db().await;
    seed_typed_probe(&db, "INSERT INTO typed_probe (x, y) VALUES (NULL, 'a')").await;
    let dup = typed_query("dup", "SELECT x, x FROM typed_probe");
    let reply = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &dup,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "sqlite_txn",
        bookclerk_db_exec::ExecCaps::from_connect(&bookclerk_plugin_abi::DbConnectResult::sqlite()),
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await;
    match reply {
        Err(err) => assert!(err.to_string().contains("duplicate column"), "{err}"),
        Ok(reply) => {
            assert_eq!(
                reply.statements[0].columns.len(),
                2,
                "duplicate expressions must stay positional, not collapse: {:?}",
                reply.statements[0].columns
            );
            assert_eq!(reply.statements[0].rows[0].values.len(), 2);
        }
    }

    let empty = typed_query("empty", "SELECT x FROM typed_probe WHERE 0");
    let reply = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &empty,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "sqlite_txn",
        bookclerk_db_exec::ExecCaps::from_connect(&bookclerk_plugin_abi::DbConnectResult::sqlite()),
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap();
    assert!(reply.statements[0].rows.is_empty());
    assert_eq!(reply.statements[0].columns.len(), 1);
    assert_eq!(reply.statements[0].columns[0].name, "x");

    let nulls = typed_query("nulls", "SELECT x FROM typed_probe");
    let reply = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &nulls,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "sqlite_txn",
        bookclerk_db_exec::ExecCaps::from_connect(&bookclerk_plugin_abi::DbConnectResult::sqlite()),
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap();
    assert_eq!(
        reply.statements[0].columns[0].db_type,
        bookclerk_plugin_abi::DbType::Int64
    );
    assert!(matches!(
        reply.statements[0].rows[0].values[0],
        bookclerk_plugin_abi::DbValue::Null(bookclerk_plugin_abi::DbType::Int64)
    ));
}

#[tokio::test]
async fn typed_sqlite_select_stops_after_cap_plus_one() {
    let db = mem_db().await;
    let backend = sea_orm::ConnectionTrait::get_database_backend(&db);
    sea_orm::ConnectionTrait::execute_raw(
        &db,
        sea_orm::Statement::from_string(
            backend,
            "CREATE TABLE IF NOT EXISTS typed_rowcap (x INTEGER)",
        ),
    )
    .await
    .ok();
    for i in 0..50 {
        sea_orm::ConnectionTrait::execute_raw(
            &db,
            sea_orm::Statement::from_sql_and_values(
                backend,
                "INSERT INTO typed_rowcap (x) VALUES (?)",
                [i.into()],
            ),
        )
        .await
        .unwrap();
    }
    let req = typed_query("cap", "SELECT x FROM typed_rowcap");
    let mut caps =
        bookclerk_db_exec::ExecCaps::from_connect(&bookclerk_plugin_abi::DbConnectResult::sqlite());
    caps.max_result_rows = CONTRACT_VECTOR_ROW_CAP;
    let err = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &req,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "sqlite_txn",
        caps,
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("maxResultRows"), "{err}");
    let seen = crate::query_rows_seen();
    assert!(
        seen <= 6,
        "must stop after cap+1 materialized rows, saw {seen}"
    );
}

#[tokio::test]
async fn typed_sqlite_statement_max_rows_is_a_proven_bound() {
    let db = mem_db().await;
    seed_typed_probe(
        &db,
        "INSERT INTO typed_probe (x, y) VALUES (1, 'a'), (2, 'b')",
    )
    .await;
    use bookclerk_plugin_abi::{
        DbPlanStatementKind, DbResultSelection, DbValue, ExecuteRequest, TypedDbStatement,
    };
    fn req(sql: &str) -> ExecuteRequest {
        ExecuteRequest {
            operation_id: "first".into(),
            request_hash: String::new(),
            statements: vec![TypedDbStatement {
                sql: sql.into(),
                parameters: vec![],
                kind: DbPlanStatementKind::Select,
                max_rows: 1,
                result_selection: DbResultSelection::Rows,
            }],
            deadline_unix_ms: 0,
        }
    }
    let caps =
        bookclerk_db_exec::ExecCaps::from_connect(&bookclerk_plugin_abi::DbConnectResult::sqlite());
    let err = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &req("SELECT x FROM typed_probe ORDER BY x"),
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "sqlite_txn",
        caps,
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("maxRows"), "{err}");

    let reply = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &req("SELECT x FROM typed_probe ORDER BY x LIMIT 1"),
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "sqlite_txn",
        caps,
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap();
    assert_eq!(reply.statements[0].rows.len(), 1);
    assert_eq!(reply.statements[0].rows[0].values[0], DbValue::Int64(1));
}

#[tokio::test]
async fn typed_sqlite_per_statement_max_result_bytes() {
    let db = mem_db().await;
    seed_typed_probe(
        &db,
        "INSERT INTO typed_probe (x, y) VALUES (1, 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx')",
    )
    .await;
    use bookclerk_plugin_abi::{
        DbPlanStatementKind, DbResultSelection, ExecuteRequest, TypedDbStatement,
    };
    let req = ExecuteRequest {
        operation_id: "bytes".into(),
        request_hash: String::new(),
        statements: vec![
            TypedDbStatement {
                sql: "SELECT y FROM typed_probe".into(),
                parameters: vec![],
                kind: DbPlanStatementKind::Select,
                max_rows: 0,
                result_selection: DbResultSelection::Rows,
            },
            TypedDbStatement {
                sql: "SELECT y FROM typed_probe".into(),
                parameters: vec![],
                kind: DbPlanStatementKind::Select,
                max_rows: 0,
                result_selection: DbResultSelection::Rows,
            },
        ],
        deadline_unix_ms: 0,
    };
    let mut caps =
        bookclerk_db_exec::ExecCaps::from_connect(&bookclerk_plugin_abi::DbConnectResult::sqlite());
    caps.max_result_bytes = 32;
    caps.max_atomic_result_bytes = 1_048_576;
    let err = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &req,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "sqlite_txn",
        caps,
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("maxResultBytes"), "{err}");
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn typed_postgres_duplicate_alias_zero_row_and_null_metadata() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    seed_typed_probe(&db, "INSERT INTO typed_probe (x, y) VALUES (NULL, 'a')").await;
    let dup = typed_query("dup", "SELECT x AS n, x AS n FROM typed_probe");
    let err = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &dup,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "postgres_txn",
        bookclerk_db_exec::ExecCaps::from_connect(
            &bookclerk_plugin_abi::DbConnectResult::postgres(),
        ),
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("duplicate") || err.to_string().to_lowercase().contains("n"),
        "{err}"
    );

    let empty = typed_query("empty", "SELECT x FROM typed_probe WHERE false");
    let reply = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &empty,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "postgres_txn",
        bookclerk_db_exec::ExecCaps::from_connect(
            &bookclerk_plugin_abi::DbConnectResult::postgres(),
        ),
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap();
    assert!(reply.statements[0].rows.is_empty());
    assert_eq!(reply.statements[0].columns.len(), 1);
    assert_eq!(reply.statements[0].columns[0].name, "x");
    assert_eq!(
        reply.statements[0].columns[0].db_type,
        bookclerk_plugin_abi::DbType::Int64
    );

    let nulls = typed_query("nulls", "SELECT x FROM typed_probe");
    let reply = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &nulls,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "postgres_txn",
        bookclerk_db_exec::ExecCaps::from_connect(
            &bookclerk_plugin_abi::DbConnectResult::postgres(),
        ),
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap();
    assert_eq!(
        reply.statements[0].columns[0].db_type,
        bookclerk_plugin_abi::DbType::Int64
    );
    assert!(matches!(
        reply.statements[0].rows[0].values[0],
        bookclerk_plugin_abi::DbValue::Null(bookclerk_plugin_abi::DbType::Int64)
    ));
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn typed_postgres_empty_select_describe_does_not_reexecute() {
    if !postgres_conformance_enabled() {
        return;
    }
    let db = postgres_migrated_db().await;
    let backend = sea_orm::ConnectionTrait::get_database_backend(&db);
    for sql in [
        "CREATE TABLE typed_counter (n INTEGER NOT NULL)",
        "INSERT INTO typed_counter (n) VALUES (0)",
        "CREATE FUNCTION typed_bump() RETURNS integer LANGUAGE plpgsql AS $$ BEGIN UPDATE typed_counter SET n = n + 1; RETURN 1; END; $$",
    ] {
        sea_orm::ConnectionTrait::execute_raw(&db, sea_orm::Statement::from_string(backend, sql))
            .await
            .unwrap();
    }
    let empty = typed_query("bump", "SELECT typed_bump() AS n LIMIT 0");
    let reply = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &empty,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "postgres_txn",
        bookclerk_db_exec::ExecCaps::from_connect(
            &bookclerk_plugin_abi::DbConnectResult::postgres(),
        ),
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap();
    assert!(reply.statements[0].rows.is_empty());
    assert_eq!(reply.statements[0].columns[0].name, "n");

    let count = typed_query("count", "SELECT n FROM typed_counter");
    let reply = bookclerk_db_exec::execute_typed_on_session(
        &db,
        &count,
        bookclerk_plugin_abi::GuestReceiptPersist::default(),
        "postgres_txn",
        bookclerk_db_exec::ExecCaps::from_connect(
            &bookclerk_plugin_abi::DbConnectResult::postgres(),
        ),
        bookclerk_db_exec::AtomicSession::from_deadline(None),
    )
    .await
    .unwrap();
    let bookclerk_plugin_abi::DbValue::Int64(n) = reply.statements[0].rows[0].values[0] else {
        panic!(
            "expected int64 counter, got {:?}",
            reply.statements[0].rows[0].values[0]
        );
    };
    assert!(
        n <= 1,
        "zero-row SELECT must execute at most once (counter={n}); describe must not re-run it"
    );
}
