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
    let env = bookclerk_db_exec::load_sql_type_env(db)
        .await
        .unwrap_or_else(|_| bookclerk_plugin_abi::SqlTypeEnv::new());
    let policy = GuestSqlPolicy::binding_owned().with_sql_types(env);
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
        err.to_string().contains("reserved")
            || err.to_string().contains("unauthorized")
            || err.to_string().contains("qualified")
            || err.to_string().contains("SQL v1"),
        "{err}"
    );
}

#[tokio::test]
async fn binding_denies_reserved_foreign_key_references() {
    let db = binding_db().await;
    let err = run_binding(
        &db,
        req(
            "fk-receipts",
            vec![stmt(
                "CREATE TABLE IF NOT EXISTS t (id INTEGER REFERENCES db_atomic_receipts(operation_id))",
                vec![],
            )],
        ),
    )
    .await
    .expect_err("FK onto host receipts must be denied");
    assert!(
        err.to_string().contains("reserved")
            || err.to_string().contains("qualified")
            || err.to_string().contains("REFERENCES"),
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
    let mut env = bookclerk_plugin_abi::SqlTypeEnv::new();
    env.insert_table(
        "counters",
        [
            ("id".into(), bookclerk_plugin_abi::SqlType::Integer),
            ("n".into(), bookclerk_plugin_abi::SqlType::Integer),
        ],
    );
    let policy = GuestSqlPolicy::binding_owned().with_sql_types(env);
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
    let env = bookclerk_db_exec::load_sql_type_env(&db)
        .await
        .unwrap_or_else(|_| bookclerk_plugin_abi::SqlTypeEnv::new());
    let policy = GuestSqlPolicy::binding_owned().with_sql_types(env);
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
    let env = bookclerk_db_exec::load_sql_type_env(&db)
        .await
        .unwrap_or_else(|_| bookclerk_plugin_abi::SqlTypeEnv::new());
    let policy = GuestSqlPolicy::binding_owned().with_sql_types(env);
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

#[tokio::test]
async fn binding_mixed_ddl_dml_applies_once_and_replays() {
    let db = binding_db().await;
    let mixed = || {
        let ddl = stmt(
            "CREATE TABLE IF NOT EXISTS counters (id INTEGER PRIMARY KEY, n INTEGER)",
            vec![],
        );
        let mut insert = stmt(
            "INSERT INTO counters (id, n) VALUES (?, ?)",
            vec![DbValue::Int64(1), DbValue::Int64(1)],
        );
        insert.result_selection = DbResultSelection::AffectedRows;
        req("mixed-once", vec![ddl, insert])
    };
    let first = run_binding(&db, mixed()).await.expect("first mixed");
    assert_eq!(first.statements.len(), 2);
    assert_eq!(first.statements[1].rows_affected, 1);
    let replay = run_binding(&db, mixed()).await.expect("replay mixed");
    assert_eq!(replay.statements[1].rows_affected, 1);
    let rows = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            "SELECT COUNT(*) AS c FROM counters",
        ))
        .await
        .expect("count");
    let count: i64 = rows[0].try_get("", "c").expect("count value");
    assert_eq!(count, 1, "mixed batch must not double-insert on replay");
}

#[tokio::test]
async fn binding_mixed_ddl_dml_preserves_gate_text_in_literal_and_comment() {
    let db = binding_db().await;
    let mixed = || {
        let ddl = stmt(bookclerk_db_exec::sql_v1::MIXED_GATE_LITERAL_DDL, vec![]);
        let mut insert = stmt(
            &bookclerk_db_exec::sql_v1::mixed_gate_literal_insert(),
            vec![],
        );
        insert.result_selection = DbResultSelection::AffectedRows;
        req("mixed-gate-lit", vec![ddl, insert])
    };
    let first = run_binding(&db, mixed()).await.expect("first mixed gate");
    assert_eq!(first.statements[1].rows_affected, 1);
    let replay = run_binding(&db, mixed()).await.expect("replay mixed gate");
    assert_eq!(replay.statements[1].rows_affected, 1);
    let mut select = stmt(bookclerk_db_exec::sql_v1::MIXED_GATE_LITERAL_SELECT, vec![]);
    select.max_rows = 8;
    let mut count = stmt(bookclerk_db_exec::sql_v1::MIXED_GATE_LITERAL_COUNT, vec![]);
    count.max_rows = 8;
    let reply = run_binding(&db, req("mixed-gate-sel", vec![select, count]))
        .await
        .expect("select mixed gate");
    assert_eq!(
        reply.statements[0].rows[0].values[0],
        DbValue::Text(bookclerk_db_exec::GUEST_RECEIPT_WRITE_GATE.into())
    );
    let DbValue::Int64(n) = reply.statements[1].rows[0].values[0] else {
        panic!(
            "expected int64 count, got {:?}",
            reply.statements[1].rows[0].values[0]
        );
    };
    assert_eq!(n, 1, "mixed gate batch must not double-insert");
}

#[tokio::test]
async fn binding_portable_functions_and_ddl_types() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-typed",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_AUTOINCREMENT_BLOB,
                vec![],
            )],
        ),
    )
    .await
    .expect("typed DDL");
    let mut insert = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_INSERT,
        vec![DbValue::Bytes(
            bookclerk_db_exec::sql_v1::PORTABLE_INSERT_BLOB.to_vec(),
        )],
    );
    insert.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("ins-typed", vec![insert]))
        .await
        .expect("typed insert");
    let mut select = stmt(bookclerk_db_exec::sql_v1::PORTABLE_SELECT, vec![]);
    select.max_rows = 8;
    let mut aggregates = stmt(bookclerk_db_exec::sql_v1::PORTABLE_AGGREGATE_SELECT, vec![]);
    aggregates.max_rows = 8;
    let reply = run_binding(&db, req("sel-typed", vec![select, aggregates]))
        .await
        .expect("portable select");
    if let Some(err) = bookclerk_db_exec::sql_v1::portable_select_mismatch(&reply.statements[0]) {
        panic!("{err}");
    }
    if let Some(err) = bookclerk_db_exec::sql_v1::portable_aggregate_mismatch(&reply.statements[1])
    {
        panic!("{err}");
    }
    let mut blob = stmt("SELECT blob FROM typed", vec![]);
    blob.max_rows = 8;
    let blob_reply = run_binding(&db, req("sel-blob", vec![blob]))
        .await
        .expect("blob select");
    assert_eq!(
        blob_reply.statements[0].rows[0].values[0],
        DbValue::Bytes(bookclerk_db_exec::sql_v1::PORTABLE_INSERT_BLOB.to_vec())
    );
}

#[tokio::test]
async fn binding_portable_boolean_column() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-bool",
            vec![stmt(bookclerk_db_exec::sql_v1::BINDING_DDL_BOOLEAN, vec![])],
        ),
    )
    .await
    .expect("boolean DDL");
    let mut insert = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_BOOLEAN_INSERT,
        bookclerk_db_exec::sql_v1::portable_boolean_insert_binds(),
    );
    insert.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("ins-bool", vec![insert]))
        .await
        .expect("boolean insert");
    let mut select = stmt(bookclerk_db_exec::sql_v1::PORTABLE_BOOLEAN_SELECT, vec![]);
    select.max_rows = 8;
    let reply = run_binding(&db, req("sel-bool", vec![select]))
        .await
        .expect("boolean select");
    if let Some(err) = bookclerk_db_exec::sql_v1::portable_boolean_mismatch(&reply.statements[0]) {
        panic!("{err}");
    }
}

#[tokio::test]
async fn binding_lowercase_ddl_and_insert_or_ignore_returning() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-lc",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_LOWERCASE,
                vec![],
            )],
        ),
    )
    .await
    .expect("lowercase DDL");
    let mut insert = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_INSERT_OR_IGNORE_RETURNING_LC,
        bookclerk_db_exec::sql_v1::portable_lowercase_insert_binds(),
    );
    insert.result_selection = DbResultSelection::Rows;
    insert.max_rows = 1;
    let inserted = run_binding(&db, req("ins-lc", vec![insert]))
        .await
        .expect("lowercase insert returning");
    let DbValue::Int64(id) = inserted.statements[0].rows[0].values[0] else {
        panic!(
            "expected int64 returning id, got {:?}",
            inserted.statements[0].rows[0].values[0]
        );
    };
    assert!(id >= 1, "returning id {id}");
    let mut select = stmt(bookclerk_db_exec::sql_v1::PORTABLE_LOWERCASE_SELECT, vec![]);
    select.max_rows = 8;
    let reply = run_binding(&db, req("sel-lc", vec![select]))
        .await
        .expect("lowercase select");
    assert_eq!(
        reply.statements[0].rows[0].values[0],
        DbValue::Bytes(bookclerk_db_exec::sql_v1::PORTABLE_INSERT_BLOB.to_vec())
    );
    assert_eq!(
        reply.statements[0].rows[0].values[1],
        DbValue::Boolean(true)
    );
}

#[tokio::test]
async fn binding_insert_or_ignore_unique_not_null_domain() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-conflict",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_CONFLICT,
                vec![],
            )],
        ),
    )
    .await
    .expect("conflict DDL");
    let mut first = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_INSERT_OR_IGNORE_UNIQUE,
        vec![],
    );
    first.result_selection = DbResultSelection::AffectedRows;
    let inserted = run_binding(&db, req("ins-u1", vec![first.clone()]))
        .await
        .expect("first unique insert");
    assert_eq!(inserted.statements[0].rows_affected, 1);
    let ignored = run_binding(&db, req("ins-u2", vec![first]))
        .await
        .expect("duplicate unique ignore");
    assert_eq!(ignored.statements[0].rows_affected, 0);
    let mut null_ins = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_INSERT_OR_IGNORE_NOT_NULL,
        vec![],
    );
    null_ins.result_selection = DbResultSelection::AffectedRows;
    let err = run_binding(&db, req("ins-nn", vec![null_ins]))
        .await
        .expect_err("NOT NULL must still abort");
    let t = err.to_string().to_ascii_lowercase();
    assert!(
        t.contains("null") || t.contains("constraint") || t.contains("not null"),
        "{err}"
    );
}

#[tokio::test]
async fn binding_min_max_null_poison_and_uncast_helpers() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-typed",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_AUTOINCREMENT_BLOB,
                vec![],
            )],
        ),
    )
    .await
    .expect("typed DDL");
    let mut insert = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_INSERT,
        vec![DbValue::Bytes(
            bookclerk_db_exec::sql_v1::PORTABLE_INSERT_BLOB.to_vec(),
        )],
    );
    insert.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("ins-typed", vec![insert]))
        .await
        .expect("typed insert");
    let mut mm = stmt(bookclerk_db_exec::sql_v1::PORTABLE_MIN_MAX_NULL, vec![]);
    mm.max_rows = 8;
    let mut round = stmt(bookclerk_db_exec::sql_v1::PORTABLE_UNCAST_ROUND, vec![]);
    round.max_rows = 8;
    let mut sum_avg = stmt(bookclerk_db_exec::sql_v1::PORTABLE_UNCAST_SUM_AVG, vec![]);
    sum_avg.max_rows = 8;
    let reply = run_binding(&db, req("sel-sem", vec![mm, round, sum_avg]))
        .await
        .expect("semantic select");
    if let Some(err) =
        bookclerk_db_exec::sql_v1::portable_min_max_null_mismatch(&reply.statements[0])
    {
        panic!("{err}");
    }
    if let Some(err) =
        bookclerk_db_exec::sql_v1::portable_uncast_round_mismatch(&reply.statements[1])
    {
        panic!("{err}");
    }
    if let Some(err) =
        bookclerk_db_exec::sql_v1::portable_uncast_sum_avg_mismatch(&reply.statements[2])
    {
        panic!("{err}");
    }
}

#[tokio::test]
async fn binding_order_by_nulls_and_identity_and_unquoted_fold() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-ord",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_ORDER_NULLS,
                vec![],
            )],
        ),
    )
    .await
    .expect("order DDL");
    for (op, sql) in [
        (
            "i1",
            bookclerk_db_exec::sql_v1::PORTABLE_ORDER_NULLS_INSERT_1,
        ),
        (
            "inull",
            bookclerk_db_exec::sql_v1::PORTABLE_ORDER_NULLS_INSERT_NULL,
        ),
        (
            "i2",
            bookclerk_db_exec::sql_v1::PORTABLE_ORDER_NULLS_INSERT_2,
        ),
    ] {
        let mut ins = stmt(sql, vec![]);
        ins.result_selection = DbResultSelection::AffectedRows;
        run_binding(&db, req(op, vec![ins])).await.expect(op);
    }
    let mut asc = stmt(bookclerk_db_exec::sql_v1::PORTABLE_ORDER_NULLS_ASC, vec![]);
    asc.max_rows = 8;
    let mut desc = stmt(bookclerk_db_exec::sql_v1::PORTABLE_ORDER_NULLS_DESC, vec![]);
    desc.max_rows = 8;
    let ordered = run_binding(&db, req("sel-ord", vec![asc, desc]))
        .await
        .expect("order select");
    if let Some(err) =
        bookclerk_db_exec::sql_v1::portable_order_nulls_asc_mismatch(&ordered.statements[0])
    {
        panic!("{err}");
    }
    if let Some(err) =
        bookclerk_db_exec::sql_v1::portable_order_nulls_desc_mismatch(&ordered.statements[1])
    {
        panic!("{err}");
    }

    run_binding(
        &db,
        req(
            "ddl-id",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_IDENTITY,
                vec![],
            )],
        ),
    )
    .await
    .expect("identity DDL");
    let mut expl = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_INSERT_EXPLICIT,
        vec![],
    );
    expl.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("ins-ex", vec![expl]))
        .await
        .expect("explicit id");
    let mut omit = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_INSERT_OMIT,
        vec![],
    );
    omit.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("ins-om", vec![omit.clone()]))
        .await
        .expect("omit id");
    let mut mx = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_SELECT_MAX,
        vec![],
    );
    mx.max_rows = 8;
    let max1 = run_binding(&db, req("sel-max1", vec![mx.clone()]))
        .await
        .expect("max after omit");
    assert_eq!(max1.statements[0].rows[0].values[0], DbValue::Int64(101));
    let mut del = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_DELETE_MAX,
        vec![],
    );
    del.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("del-max", vec![del]))
        .await
        .expect("delete max");
    run_binding(&db, req("ins-om2", vec![omit]))
        .await
        .expect("omit after delete");
    let max2 = run_binding(&db, req("sel-max2", vec![mx]))
        .await
        .expect("max after reinsert");
    assert_eq!(max2.statements[0].rows[0].values[0], DbValue::Int64(102));

    run_binding(
        &db,
        req(
            "ddl-fold",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_UNQUOTED_FOLD,
                vec![],
            )],
        ),
    )
    .await
    .expect("fold DDL");
    let mut fins = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_UNQUOTED_FOLD_INSERT,
        vec![],
    );
    fins.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("ins-fold", vec![fins]))
        .await
        .expect("fold insert");
    let mut fsel = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_UNQUOTED_FOLD_SELECT,
        vec![],
    );
    fsel.max_rows = 8;
    let folded = run_binding(&db, req("sel-fold", vec![fsel]))
        .await
        .expect("fold select");
    assert_eq!(folded.statements[0].rows[0].values[0], DbValue::Int64(7));
}

#[tokio::test]
async fn binding_sql_v1_p1_vectors() {
    let db = binding_db().await;
    run_binding(
        &db,
        req(
            "ddl-ign",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_IGNORE_SELECT,
                vec![],
            )],
        ),
    )
    .await
    .expect("ign ddl");
    for (op, sql, binds) in [
        (
            "ign-sel",
            bookclerk_db_exec::sql_v1::PORTABLE_IGNORE_SELECT,
            vec![DbValue::Int64(1)],
        ),
        (
            "ign-with",
            bookclerk_db_exec::sql_v1::PORTABLE_IGNORE_SELECT_WITH,
            vec![DbValue::Int64(2)],
        ),
        (
            "ign-union",
            bookclerk_db_exec::sql_v1::PORTABLE_IGNORE_SELECT_UNION,
            vec![DbValue::Int64(3), DbValue::Int64(4)],
        ),
        (
            "ign-ord",
            bookclerk_db_exec::sql_v1::PORTABLE_IGNORE_SELECT_ORDER_LIMIT,
            vec![DbValue::Int64(5)],
        ),
    ] {
        let mut ins = stmt(sql, binds);
        ins.result_selection = DbResultSelection::Rows;
        ins.max_rows = 0;
        run_binding(&db, req(op, vec![ins])).await.expect(op);
    }

    run_binding(
        &db,
        req(
            "ddl-like",
            vec![stmt(bookclerk_db_exec::sql_v1::BINDING_DDL_LIKE, vec![])],
        ),
    )
    .await
    .expect("like ddl");
    let mut like_ins = stmt(bookclerk_db_exec::sql_v1::PORTABLE_LIKE_INSERT, vec![]);
    like_ins.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("like-ins", vec![like_ins]))
        .await
        .expect("like ins");
    let mut like_sel = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_LIKE_SELECT,
        vec![DbValue::Text("A".into())],
    );
    like_sel.max_rows = 8;
    let liked = run_binding(&db, req("like-sel", vec![like_sel]))
        .await
        .expect("like sel");
    if let Some(err) = bookclerk_db_exec::sql_v1::portable_statement_mismatch(
        &liked.statements[0],
        bookclerk_db_exec::sql_v1::portable_like_expects(),
        "like",
    ) {
        panic!("{err}");
    }
    let mut like_na = stmt(bookclerk_db_exec::sql_v1::PORTABLE_LIKE_NON_ASCII, vec![]);
    like_na.max_rows = 8;
    let na = run_binding(&db, req("like-na", vec![like_na]))
        .await
        .expect("like na");
    assert_eq!(na.statements[0].rows[0].values[0], DbValue::Int64(0));

    run_binding(
        &db,
        req(
            "ddl-blobdef",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_BLOB_DEFAULT,
                vec![],
            )],
        ),
    )
    .await
    .expect("blobdef ddl");
    let mut bdi = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_BLOB_DEFAULT_INSERT,
        vec![],
    );
    bdi.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("blobdef-ins", vec![bdi]))
        .await
        .expect("blobdef ins");
    let mut bds = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_BLOB_DEFAULT_SELECT,
        vec![],
    );
    bds.max_rows = 8;
    let blob = run_binding(&db, req("blobdef-sel", vec![bds]))
        .await
        .expect("blobdef sel");
    assert_eq!(
        blob.statements[0].rows[0].values[0],
        DbValue::Bytes(vec![0xde, 0xad, 0xbe, 0xef])
    );

    run_binding(
        &db,
        req(
            "ddl-textord",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_TEXT_ORDER,
                vec![],
            )],
        ),
    )
    .await
    .expect("textord ddl");
    for (op, sql) in [
        (
            "to-b",
            bookclerk_db_exec::sql_v1::PORTABLE_TEXT_ORDER_INSERT_B,
        ),
        (
            "to-a",
            bookclerk_db_exec::sql_v1::PORTABLE_TEXT_ORDER_INSERT_A,
        ),
        (
            "to-eac",
            bookclerk_db_exec::sql_v1::PORTABLE_TEXT_ORDER_INSERT_EACUTE,
        ),
        (
            "to-e",
            bookclerk_db_exec::sql_v1::PORTABLE_TEXT_ORDER_INSERT_E,
        ),
    ] {
        let mut ins = stmt(sql, vec![]);
        ins.result_selection = DbResultSelection::AffectedRows;
        run_binding(&db, req(op, vec![ins])).await.expect(op);
    }
    let mut tos = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_TEXT_ORDER_SELECT,
        vec![],
    );
    tos.max_rows = 8;
    let ordered = run_binding(&db, req("text-ord", vec![tos]))
        .await
        .expect("text ord");
    if let Some(err) = bookclerk_db_exec::sql_v1::portable_rows_mismatch(
        &ordered.statements[0],
        bookclerk_db_exec::sql_v1::portable_text_order_expects(),
        "text order",
    ) {
        panic!("{err}");
    }
    let mut ops = stmt(bookclerk_db_exec::sql_v1::PORTABLE_TEXT_OPS, vec![]);
    ops.max_rows = 8;
    let tops = run_binding(&db, req("text-ops", vec![ops]))
        .await
        .expect("text ops");
    if let Some(err) = bookclerk_db_exec::sql_v1::portable_statement_mismatch(
        &tops.statements[0],
        bookclerk_db_exec::sql_v1::portable_text_ops_expects(),
        "text ops",
    ) {
        panic!("{err}");
    }

    // Identity rollback: explicit 100 + unique conflict rolls back; omit-id is 1.
    run_binding(
        &db,
        req(
            "ddl-ident",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_IDENTITY,
                vec![],
            )],
        ),
    )
    .await
    .expect("ident ddl");
    let mut ok = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_INSERT_EXPLICIT,
        vec![],
    );
    ok.result_selection = DbResultSelection::AffectedRows;
    let mut dup = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_INSERT_EXPLICIT,
        vec![],
    );
    dup.result_selection = DbResultSelection::AffectedRows;
    let err = run_binding(&db, req("ident-rollback", vec![ok, dup]))
        .await
        .expect_err("unique conflict must abort the batch");
    assert!(!err.to_string().is_empty(), "{err}");
    let mut omit = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_INSERT_OMIT,
        vec![],
    );
    omit.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("ident-omit-after-rollback", vec![omit]))
        .await
        .expect("omit after rollback");
    let mut mx = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_SELECT_MAX,
        vec![],
    );
    mx.max_rows = 8;
    let maxed = run_binding(&db, req("ident-max-rb", vec![mx]))
        .await
        .expect("max after rollback");
    assert_eq!(maxed.statements[0].rows[0].values[0], DbValue::Int64(1));

    run_binding(
        &db,
        req(
            "ident-drop",
            vec![stmt(
                bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_DROP,
                vec![],
            )],
        ),
    )
    .await
    .expect("drop ident");
    run_binding(
        &db,
        req(
            "ident-recreate",
            vec![stmt(
                bookclerk_db_exec::sql_v1::BINDING_DDL_IDENTITY,
                vec![],
            )],
        ),
    )
    .await
    .expect("recreate ident");
    let mut omit2 = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_INSERT_OMIT,
        vec![],
    );
    omit2.result_selection = DbResultSelection::AffectedRows;
    run_binding(&db, req("ident-omit-recreate", vec![omit2]))
        .await
        .expect("omit after recreate");
    let mut mx2 = stmt(
        bookclerk_db_exec::sql_v1::PORTABLE_IDENTITY_SELECT_MAX,
        vec![],
    );
    mx2.max_rows = 8;
    let maxed2 = run_binding(&db, req("ident-max-re", vec![mx2]))
        .await
        .expect("max after recreate");
    assert_eq!(maxed2.statements[0].rows[0].values[0], DbValue::Int64(1));

    // Reopen: catalog rows survive a fresh policy load (no in-request CREATE).
    run_binding(
        &db,
        req(
            "ddl-reopen",
            vec![stmt(
                "CREATE TABLE IF NOT EXISTS typed_reopen (n INTEGER, body TEXT)",
                vec![],
            )],
        ),
    )
    .await
    .expect("reopen ddl");
    let env = bookclerk_db_exec::load_sql_type_env(&db)
        .await
        .expect("load catalog");
    assert_eq!(
        env.column_type("typed_reopen", "body"),
        Some(bookclerk_plugin_abi::SqlType::Text)
    );
    assert_eq!(
        env.column_type("typed_reopen", "n"),
        Some(bookclerk_plugin_abi::SqlType::Integer)
    );
    let policy = GuestSqlPolicy::binding_owned().with_sql_types(env);
    let missing = req(
        "reopen-missing",
        vec![stmt("SELECT missing FROM typed_reopen", vec![])],
    );
    bookclerk_plugin_abi::validate_guest_execute_request_for_policy(&missing, &policy)
        .expect_err("unknown column after catalog reload");
    let mixed = req(
        "reopen-mixed",
        vec![stmt("SELECT IFNULL(body, n) FROM typed_reopen", vec![])],
    );
    bookclerk_plugin_abi::validate_guest_execute_request_for_policy(&mixed, &policy)
        .expect_err("IFNULL mixed columns after catalog reload");
    let ok_sel = req(
        "reopen-ok",
        vec![stmt("SELECT body FROM typed_reopen", vec![])],
    );
    bookclerk_plugin_abi::validate_guest_execute_request_for_policy(&ok_sel, &policy)
        .expect("catalog types admit TEXT select");

    // Expression-only typecheck (empty env): mixed literals still fail closed.
    let err = bookclerk_library::execute_guest_atomic_with(
        req("ifnull-mixed", vec![stmt("SELECT IFNULL('x', 0)", vec![])]),
        &DbCapabilities::advertised_sqlite(),
        &GuestSqlPolicy::binding_owned(),
        |_| async { unreachable!("typecheck must reject") },
    )
    .await
    .expect_err("IFNULL mixed types");
    assert!(
        err.to_string().contains("incompatible") || err.to_string().contains("invalid"),
        "{err}"
    );
}
