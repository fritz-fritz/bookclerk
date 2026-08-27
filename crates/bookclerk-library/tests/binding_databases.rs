//! Acceptance tests for isolated plugin database bindings.
//!
//! Drives [`bookclerk_library::execute_guest_atomic_with`] — the exact host
//! path behind a binding `GuestDatabase` — against dedicated SQLite
//! connections, proving plugin-owned DDL, cross-binding isolation, reserved
//! table denial, and retry-token replay inside the binding.

use bookclerk_plugin_abi::{
    DbCapabilities, DbPlanStatementKind, DbResultSelection, DbValue, ExecuteReply, ExecuteRequest,
    GuestSqlPolicy, PluginError, TypedDbStatement,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// One isolated binding database with its receipt bootstrap applied.
async fn binding_db() -> DatabaseConnection {
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .expect("in-memory binding database");
    for sql in bookclerk_db_exec::split_schema_statements(
        bookclerk_library::migrations::binding_bootstrap_sql(),
    ) {
        db.execute_raw(Statement::from_string(db.get_database_backend(), sql))
            .await
            .expect("binding bootstrap DDL");
    }
    db
}

/// Executes one guest request through the binding authorization + receipt path.
async fn run_binding(
    db: &DatabaseConnection,
    request: ExecuteRequest,
) -> Result<ExecuteReply, PluginError> {
    let caps = DbCapabilities::advertised_sqlite();
    let policy = GuestSqlPolicy::binding_owned();
    bookclerk_library::execute_guest_atomic_with(request, &caps, &policy, |envelope| async move {
        let deadline =
            (envelope.request.deadline_unix_ms > 0).then_some(envelope.request.deadline_unix_ms);
        bookclerk_db_exec::execute_typed_on_session(
            db,
            &envelope.request,
            envelope.guest_receipt,
            "sqlite_txn",
            bookclerk_db_exec::ExecCaps::from_capabilities(&DbCapabilities::advertised_sqlite()),
            bookclerk_db_exec::AtomicSession::from_deadline(deadline),
        )
        .await
        .map_err(|err| PluginError::internal(err.to_string()))
    })
    .await
}

fn stmt(sql: &str, parameters: Vec<DbValue>) -> TypedDbStatement {
    let kind = bookclerk_plugin_abi::guest_statement_kind(sql);
    let result_selection = if kind == DbPlanStatementKind::Select {
        DbResultSelection::Rows
    } else {
        DbResultSelection::Discard
    };
    TypedDbStatement {
        sql: sql.into(),
        parameters,
        kind,
        max_rows: 0,
        result_selection,
    }
}

fn req(operation_id: &str, statements: Vec<TypedDbStatement>) -> ExecuteRequest {
    ExecuteRequest {
        operation_id: operation_id.into(),
        request_hash: String::new(),
        statements,
        deadline_unix_ms: 0,
    }
}

#[tokio::test]
async fn binding_owns_schema_and_stays_isolated_from_its_sibling() {
    let a = binding_db().await;
    let b = binding_db().await;

    // Each binding creates the same-named table and inserts its own row —
    // plugin-owned DDL through the guest path, isolated per binding.
    for (db, marker) in [(&a, "alpha"), (&b, "beta")] {
        run_binding(
            db,
            req(
                &format!("ddl-{marker}"),
                vec![stmt(
                    "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT)",
                    vec![],
                )],
            ),
        )
        .await
        .expect("binding DDL");
        run_binding(
            db,
            req(
                &format!("ins-{marker}"),
                vec![stmt(
                    "INSERT INTO notes (body) VALUES (?)",
                    vec![DbValue::Text(marker.to_string())],
                )],
            ),
        )
        .await
        .expect("binding insert");
    }

    for (db, marker) in [(&a, "alpha"), (&b, "beta")] {
        let mut select = stmt("SELECT body FROM notes ORDER BY id", vec![]);
        select.max_rows = 10;
        let reply = run_binding(db, req(&format!("sel-{marker}"), vec![select]))
            .await
            .expect("binding select");
        let rows = &reply.statements[0].rows;
        assert_eq!(rows.len(), 1, "each binding sees only its own rows");
        assert_eq!(rows[0].values[0], DbValue::Text(marker.to_string()));
    }
}

#[tokio::test]
async fn binding_denies_reserved_tables_and_qualified_names() {
    let db = binding_db().await;
    let err = run_binding(
        &db,
        req(
            "reserved",
            vec![stmt("SELECT operation_id FROM db_atomic_receipts", vec![])],
        ),
    )
    .await
    .expect_err("receipt table must stay host-owned");
    assert!(
        err.to_string().contains("reserved") || err.to_string().contains("unauthorized"),
        "{err}"
    );
    let err = run_binding(
        &db,
        req(
            "qualified",
            vec![stmt("SELECT id FROM main.sqlite_master", vec![])],
        ),
    )
    .await
    .expect_err("qualified names must be denied");
    assert!(
        err.to_string().contains("reserved") || err.to_string().contains("unauthorized"),
        "{err}"
    );
}

#[tokio::test]
async fn binding_retry_token_replays_without_double_apply() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "setup",
            vec![stmt(
                "CREATE TABLE IF NOT EXISTS counters (id INTEGER PRIMARY KEY, n INTEGER)",
                vec![],
            )],
        ),
    )
    .await
    .expect("setup DDL");

    let insert = || {
        let mut write = stmt(
            "INSERT INTO counters (id, n) VALUES (?, ?)",
            vec![DbValue::Int64(1), DbValue::Int64(1)],
        );
        write.result_selection = DbResultSelection::AffectedRows;
        req("op-once", vec![write])
    };
    let first = run_binding(&db, insert()).await.expect("first apply");
    assert_eq!(first.statements[0].rows_affected, 1);
    let replay = run_binding(&db, insert()).await.expect("same-token retry");
    assert_eq!(replay.statements[0].rows_affected, 1);
    let rows = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            "SELECT COUNT(*) AS c FROM counters",
        ))
        .await
        .expect("count");
    let count: i64 = rows[0].try_get("", "c").expect("count value");
    assert_eq!(count, 1, "receipt gate must prevent a double apply");
}

#[tokio::test]
async fn binding_ddl_hash_mismatch_does_not_change_schema() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-op",
            vec![stmt(
                "CREATE TABLE IF NOT EXISTS alpha (id INTEGER PRIMARY KEY)",
                vec![],
            )],
        ),
    )
    .await
    .expect("first DDL");

    let err = run_binding(
        &db,
        req(
            "ddl-op",
            vec![stmt(
                "CREATE TABLE IF NOT EXISTS beta (id INTEGER PRIMARY KEY)",
                vec![],
            )],
        ),
    )
    .await
    .expect_err("changed hash must conflict");
    assert_eq!(err.code, bookclerk_plugin_abi::PluginErrorCode::Conflict);

    let alpha = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'alpha'",
        ))
        .await
        .expect("alpha");
    assert_eq!(alpha.len(), 1, "original table must remain");
    let beta = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'beta'",
        ))
        .await
        .expect("beta");
    assert!(beta.is_empty(), "mismatched DDL must not create beta");
}

#[tokio::test]
async fn binding_session_caps_are_enforced_independently_of_library_caps() {
    let mut binding_caps = DbCapabilities::advertised_sqlite();
    binding_caps.max_binds = 1;
    let library_caps = DbCapabilities::advertised_sqlite();
    assert!(library_caps.max_binds > 1);
    let policy = GuestSqlPolicy::binding_owned();
    let request = req(
        "two-binds",
        vec![stmt(
            "INSERT INTO counters (id, n) VALUES (?, ?)",
            vec![DbValue::Int64(1), DbValue::Int64(1)],
        )],
    );
    let err = bookclerk_library::execute_guest_atomic_with(
        request,
        &binding_caps,
        &policy,
        |_envelope| async move { unreachable!("must reject before exec") },
    )
    .await
    .expect_err("binding maxBinds=1 must reject two binds");
    assert!(
        err.to_string().to_lowercase().contains("bind")
            || err.to_string().to_lowercase().contains("max"),
        "{err}"
    );
}

#[tokio::test]
async fn binding_cancel_before_begin_does_not_commit() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-notes",
            vec![stmt(
                "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT)",
                vec![],
            )],
        ),
    )
    .await
    .expect("create notes");
    let cancel = Arc::new(AtomicBool::new(true));
    let caps = DbCapabilities::advertised_sqlite();
    let policy = GuestSqlPolicy::binding_owned();
    let request = req(
        "insert-notes",
        vec![stmt(
            "INSERT INTO notes (body) VALUES (?)",
            vec![DbValue::Text("should-not-commit".into())],
        )],
    );
    let err = bookclerk_library::execute_guest_atomic_with(request, &caps, &policy, |envelope| {
        let cancel = Arc::clone(&cancel);
        let db = db.clone();
        async move {
            let deadline = (envelope.request.deadline_unix_ms > 0)
                .then_some(envelope.request.deadline_unix_ms);
            bookclerk_db_exec::execute_typed_on_session(
                    &db,
                    &envelope.request,
                    envelope.guest_receipt,
                    "sqlite_txn",
                    bookclerk_db_exec::ExecCaps::from_capabilities(
                        &DbCapabilities::advertised_sqlite(),
                    ),
                    bookclerk_db_exec::AtomicSession::from_deadline(deadline)
                        .with_cancel(Some(cancel)),
                )
                .await
                .map_err(|err| PluginError::internal(err.to_string()))
        }
    })
    .await
    .expect_err("cancelled session must not commit");
    assert!(err.to_string().to_lowercase().contains("cancel"), "{err}");
    let rows = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            "SELECT COUNT(*) AS n FROM notes".to_string(),
        ))
        .await
        .expect("count");
    let n: i64 = rows[0].try_get("", "n").expect("n");
    assert_eq!(n, 0, "cancelled insert must roll back");
}

#[tokio::test]
async fn binding_cancel_around_commit_rolls_back() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-notes-2",
            vec![stmt(
                "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT)",
                vec![],
            )],
        ),
    )
    .await
    .expect("create notes");
    bookclerk_db_exec::inject_atomic_interrupt(
        bookclerk_db_exec::AtomicInterruptPhase::AroundCommit,
        bookclerk_db_exec::AtomicInterruptKind::Cancel,
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let caps = DbCapabilities::advertised_sqlite();
    let policy = GuestSqlPolicy::binding_owned();
    let request = req(
        "insert-notes-2",
        vec![stmt(
            "INSERT INTO notes (body) VALUES (?)",
            vec![DbValue::Text("around-commit".into())],
        )],
    );
    let err = bookclerk_library::execute_guest_atomic_with(request, &caps, &policy, |envelope| {
        let cancel = Arc::clone(&cancel);
        let db = db.clone();
        async move {
            let deadline = (envelope.request.deadline_unix_ms > 0)
                .then_some(envelope.request.deadline_unix_ms);
            bookclerk_db_exec::execute_typed_on_session(
                    &db,
                    &envelope.request,
                    envelope.guest_receipt,
                    "sqlite_txn",
                    bookclerk_db_exec::ExecCaps::from_capabilities(
                        &DbCapabilities::advertised_sqlite(),
                    ),
                    bookclerk_db_exec::AtomicSession::from_deadline(deadline)
                        .with_cancel(Some(cancel)),
                )
                .await
                .map_err(|err| PluginError::internal(err.to_string()))
        }
    })
    .await
    .expect_err("AroundCommit cancel must not commit");
    assert!(
        err.to_string().to_lowercase().contains("commit")
            || err.to_string().to_lowercase().contains("cancel")
            || err.to_string().to_lowercase().contains("interrupt"),
        "{err}"
    );
    let rows = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            "SELECT COUNT(*) AS n FROM notes".to_string(),
        ))
        .await
        .expect("count");
    let n: i64 = rows[0].try_get("", "n").expect("n");
    assert_eq!(n, 0, "AroundCommit cancel must roll back");
}
