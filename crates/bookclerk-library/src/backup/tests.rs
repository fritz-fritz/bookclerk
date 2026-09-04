//! Backup repository, streaming capture, restore, and schema-state tests.

#![allow(clippy::missing_docs_in_private_items)]

use super::*;
use crate::backup::encode::{
    decode_canonical_object, encode_canonical_object, sha256_hex, unwrap_stored_object,
    wrap_stored_object, CanonicalObject, CHUNK_TARGET_UNCOMPRESSED_BYTES,
};
use crate::backup::repository::BackupRepository;
use crate::backup::restore::{apply_admitted_sql, restore_backup_unit};
use crate::backup::schema::{
    admit_canonical_schema, canonical_order_by_sql, library_ddl_for_schema_state_with,
    order_key_columns, sort_tables_by_foreign_keys,
};
use crate::backup::util::validate_cell;
use crate::backup::verify::verify_recovery_point;
use crate::host_schema::{apply_host_schema, current_schema_state, HostSchemaKind};
use crate::migrations::{HostMigrationStep, SCHEMA_MIGRATIONS_DDL, SCHEMA_VERSION};
use crate::LibraryStore;
use bookclerk_plugin_abi::{
    encoded_statement_result_bytes, parse_create_table_schema, DbColumn, DbRow, DbType, DbValue,
    SqlType, StatementResult, FIRST_PARTY_MAX_RESULT_BYTES, FIRST_PARTY_MAX_RESULT_ROWS,
    SQL_CONTRACT_VERSION,
};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn backup_req(files: &Path, state: SchemaState, reason: BackupReason) -> BackupRequest {
    BackupRequest {
        files_dir: files.to_path_buf(),
        schema_state: state,
        reason,
        to_version: 0,
        include_plugin_databases: false,
        consistent_backup_read: true,
        backend_at_capture: "sqlite".into(),
        max_result_rows: FIRST_PARTY_MAX_RESULT_ROWS,
        max_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
        max_atomic_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
        plugin_units: Vec::new(),
    }
}

fn restore_ok() -> CanonicalRestoreOpts {
    CanonicalRestoreOpts::default()
}

fn restore_without_atomic() -> CanonicalRestoreOpts {
    CanonicalRestoreOpts {
        atomic_unit_restore: false,
        ..CanonicalRestoreOpts::default()
    }
}

fn prepared_plugin(
    plugin_id: &str,
    binding: &str,
    db: sea_orm::DatabaseConnection,
) -> PreparedPluginUnit {
    PreparedPluginUnit {
        plugin_id: plugin_id.into(),
        binding: binding.into(),
        backend_at_capture: "sqlite".into(),
        db,
        max_result_rows: FIRST_PARTY_MAX_RESULT_ROWS,
        max_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
        max_atomic_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
    }
}

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_all_raw(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap()
        .first()
        .and_then(|row| {
            row.try_get::<i64>("", "count")
                .ok()
                .or_else(|| row.try_get_by_index::<i64>(0).ok())
        })
        .unwrap_or(0)
}

async fn apply_bootstrap(db: &DatabaseConnection) {
    for stmt in
        bookclerk_db_exec::split_schema_statements(crate::migrations::binding_bootstrap_sql())
    {
        db.execute_raw(Statement::from_string(DbBackend::Sqlite, stmt))
            .await
            .unwrap();
    }
}

#[test]
fn identical_canonical_chunks_hash_identically() {
    let a = CanonicalObject::TableChunk {
        table: "t".into(),
        columns: vec!["id".into()],
        rows: vec![vec![DbValue::Int64(1)]],
    };
    let b = a.clone();
    assert_eq!(
        sha256_hex(&encode_canonical_object(&a).unwrap()),
        sha256_hex(&encode_canonical_object(&b).unwrap())
    );
}

#[test]
fn gzip_envelope_preserves_uncompressed_digest() {
    let raw = encode_canonical_object(&CanonicalObject::Identity {
        entries: BTreeMap::new(),
    })
    .unwrap();
    let stored = wrap_stored_object(&raw).unwrap();
    assert_eq!(unwrap_stored_object(&stored).unwrap(), raw);
    assert_eq!(
        decode_canonical_object(&raw).unwrap(),
        CanonicalObject::Identity {
            entries: BTreeMap::new()
        }
    );
    const {
        assert!(CHUNK_TARGET_UNCOMPRESSED_BYTES >= 128 * 1024);
        assert!(CHUNK_TARGET_UNCOMPRESSED_BYTES <= 512 * 1024);
    }
}

#[test]
fn admit_rejects_unparseable_schema() {
    let err =
        admit_canonical_schema(SQL_CONTRACT_VERSION, "CREATE VIEW v AS SELECT 1").unwrap_err();
    assert!(err.to_string().contains("not fully admitted"), "{err}");
}

#[test]
fn fk_order_parents_before_reverse_alphabetical_children() {
    let sql = r#"
        CREATE TABLE a_child (
            id INTEGER PRIMARY KEY,
            parent_id INTEGER NOT NULL REFERENCES z_parent(id)
        );
        CREATE TABLE z_parent (id INTEGER PRIMARY KEY);
    "#;
    let schema = admit_canonical_schema(SQL_CONTRACT_VERSION, sql).unwrap();
    let ordered = sort_tables_by_foreign_keys(schema.tables).unwrap();
    assert_eq!(ordered[0].parsed.table, "z_parent");
    assert_eq!(ordered[1].parsed.table, "a_child");
}

#[test]
fn admit_covers_pk_unique_fk_index_default_and_check() {
    let sql = r#"
        CREATE TABLE z_parent (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL DEFAULT 'x',
            flag BOOLEAN NOT NULL CHECK (flag = TRUE OR flag = FALSE)
        );
        CREATE TABLE a_child (
            id INTEGER PRIMARY KEY,
            parent_id INTEGER NOT NULL,
            UNIQUE (parent_id),
            FOREIGN KEY (parent_id) REFERENCES z_parent (id)
        );
        CREATE INDEX idx_child_parent ON a_child (parent_id);
    "#;
    let schema = admit_canonical_schema(SQL_CONTRACT_VERSION, sql).unwrap();
    let ordered = sort_tables_by_foreign_keys(schema.tables.clone()).unwrap();
    assert_eq!(ordered[0].parsed.table, "z_parent");
    assert_eq!(ordered[1].parsed.table, "a_child");
    assert_eq!(schema.indexes.len(), 1);
    assert_eq!(schema.indexes[0].name, "idx_child_parent");
}

#[test]
fn fk_cycle_fails_closed() {
    let sql = r#"
        CREATE TABLE a (id INTEGER PRIMARY KEY, b_id INTEGER REFERENCES b(id));
        CREATE TABLE b (id INTEGER PRIMARY KEY, a_id INTEGER REFERENCES a(id));
    "#;
    let schema = admit_canonical_schema(SQL_CONTRACT_VERSION, sql).unwrap();
    let err = sort_tables_by_foreign_keys(schema.tables).unwrap_err();
    assert!(err.to_string().contains("cyclic"), "{err}");
}

#[test]
fn order_key_prefers_pk_then_unique_then_full_row() {
    let pk =
        parse_create_table_schema("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .unwrap();
    assert_eq!(
        order_key_columns(&pk),
        vec!["id".to_string(), "name".to_string()]
    );
    assert_eq!(
        canonical_order_by_sql(&pk),
        "id ASC NULLS FIRST, name ASC NULLS FIRST"
    );
    let uniq = parse_create_table_schema("CREATE TABLE t (a TEXT, b TEXT, UNIQUE (b, a))").unwrap();
    assert_eq!(
        order_key_columns(&uniq),
        vec!["b".to_string(), "a".to_string()]
    );
    let one_unique =
        parse_create_table_schema("CREATE TABLE t (k TEXT UNIQUE, extra TEXT)").unwrap();
    assert_eq!(
        order_key_columns(&one_unique),
        vec!["k".to_string(), "extra".to_string()]
    );
    let keyless = parse_create_table_schema("CREATE TABLE t (a TEXT, b TEXT)").unwrap();
    assert_eq!(
        order_key_columns(&keyless),
        vec!["a".to_string(), "b".to_string()]
    );
}

#[test]
fn restore_insert_sql_lowers_question_marks_on_postgres() {
    let sql = "INSERT INTO t (a, b) VALUES (?, ?)";
    assert_eq!(
        bookclerk_db_exec::lower_canonical_sql(DbBackend::Postgres, sql),
        "INSERT INTO t (a, b) VALUES ($1, $2)"
    );
}

#[test]
fn put_object_replaces_corrupt_existing_digest() {
    let files = tempfile::tempdir().unwrap();
    let repo = BackupRepository::open(files.path()).unwrap();
    let object = CanonicalObject::Identity {
        entries: BTreeMap::new(),
    };
    let digest = repo.put_object(&object).unwrap();
    let path = files
        .path()
        .join(BACKUPS_DIR)
        .join("objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    std::fs::write(&path, b"truncated").unwrap();
    assert!(repo.get_object(&digest).is_err());
    assert_eq!(repo.put_object(&object).unwrap(), digest);
    match repo.get_object(&digest).unwrap() {
        CanonicalObject::Identity { entries } => assert!(entries.is_empty()),
        other => panic!("expected Identity, got {other:?}"),
    }
}

#[test]
fn validate_cell_enforces_types_and_nullability() {
    validate_cell(
        "t",
        "n",
        &DbValue::Null(DbType::Int64),
        SqlType::Integer,
        false,
    )
    .unwrap();
    let err = validate_cell(
        "t",
        "n",
        &DbValue::Null(DbType::Int64),
        SqlType::Integer,
        true,
    )
    .unwrap_err();
    assert!(err.to_string().contains("NOT NULL"), "{err}");
    validate_cell("t", "b", &DbValue::Boolean(true), SqlType::Boolean, true).unwrap();
    validate_cell("t", "i", &DbValue::Int64(i64::MIN), SqlType::Integer, true).unwrap();
    validate_cell("t", "i", &DbValue::Int64(i64::MAX), SqlType::Integer, true).unwrap();
    validate_cell("t", "f", &DbValue::Float64(1.25), SqlType::Real, true).unwrap();
    let err =
        validate_cell("t", "f", &DbValue::Float64(f64::NAN), SqlType::Real, true).unwrap_err();
    assert!(err.to_string().contains("non-finite"), "{err}");
    let err = validate_cell(
        "t",
        "f",
        &DbValue::Float64(f64::INFINITY),
        SqlType::Real,
        true,
    )
    .unwrap_err();
    assert!(err.to_string().contains("non-finite"), "{err}");
    validate_cell("t", "s", &DbValue::Text("🦀".into()), SqlType::Text, true).unwrap();
    validate_cell(
        "t",
        "blob",
        &DbValue::Bytes(vec![0, 255]),
        SqlType::Blob,
        true,
    )
    .unwrap();
    let err = validate_cell("t", "s", &DbValue::Int64(1), SqlType::Text, true).unwrap_err();
    assert!(err.to_string().contains("does not match"), "{err}");
}

#[test]
fn schema_state_frozen_uses_that_version_not_latest() {
    let v1 = HostMigrationStep {
        version: 1,
        canonical: "CREATE TABLE frozen_v1 (id INTEGER PRIMARY KEY);",
        down: None,
        introduced_in: "0.1.0",
    };
    let v2 = HostMigrationStep {
        version: 2,
        canonical: "CREATE TABLE frozen_v2 (id INTEGER PRIMARY KEY);",
        down: None,
        introduced_in: "0.2.0",
    };
    let plan = [v1, v2];
    let checksum = plan[0].checksum();
    let sql = library_ddl_for_schema_state_with(
        &plan,
        "CREATE TABLE unreleased_only (id INTEGER PRIMARY KEY);",
        "deadbeef",
        SCHEMA_MIGRATIONS_DDL,
        2,
        &SchemaState::Frozen {
            version: 1,
            checksum,
        },
    )
    .unwrap();
    assert!(sql.contains("frozen_v1"));
    assert!(!sql.contains("frozen_v2"));
    assert!(!sql.contains("unreleased_only"));
}

#[test]
fn library_current_schema_admits_after_filtering_seed_dml() {
    let schema = library_canonical_schema().expect("current library pack must admit");
    assert!(
        schema.tables.iter().any(|t| t.parsed.table == "accounts"),
        "expected accounts table"
    );
    let joined = schema.schema_sql().join("\n").to_ascii_uppercase();
    assert!(
        !joined.contains("INSERT"),
        "seed DML must not live in the schema object"
    );
}

#[test]
fn schema_state_unreleased_includes_matching_pack() {
    let unreleased = "CREATE TABLE extra (id INTEGER PRIMARY KEY);";
    let checksum = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(unreleased.as_bytes()))
    };
    let sql = library_ddl_for_schema_state_with(
        &[],
        unreleased,
        &checksum,
        SCHEMA_MIGRATIONS_DDL,
        SCHEMA_VERSION,
        &SchemaState::Unreleased {
            base_version: SCHEMA_VERSION,
            checksum: checksum.clone(),
        },
    )
    .unwrap();
    assert!(sql.contains("extra"), "{sql}");
    assert!(
        sql.to_ascii_lowercase().contains("schema_migrations"),
        "{sql}"
    );
}

#[tokio::test]
async fn uninitialized_skips_even_with_include_plugin_databases() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    let skip = backup_library(
        &db,
        &BackupRequest {
            files_dir: files.path().to_path_buf(),
            schema_state: SchemaState::Uninitialized,
            reason: BackupReason::Manual,
            to_version: 0,
            include_plugin_databases: true,
            consistent_backup_read: true,
            backend_at_capture: "sqlite".into(),
            max_result_rows: 100,
            max_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_atomic_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            plugin_units: Vec::new(),
        },
    )
    .await
    .unwrap();
    assert!(skip.is_none());
}

#[tokio::test]
async fn sqlite_library_round_trip_replaces_not_merges() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
         VALUES ('keep', 'us', 'audible', 't', 't')",
    ))
    .await
    .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let outcome = backup_library(&db, &backup_req(files.path(), state, BackupReason::Manual))
        .await
        .unwrap()
        .expect("unreleased backup");
    assert!(outcome.manifest.schema_state.starts_with("unreleased@"));
    assert_eq!(outcome.manifest.reason, BackupReason::Manual);
    assert_eq!(outcome.manifest.format_version, BACKUP_FORMAT_VERSION);

    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
         VALUES ('extra', 'us', 'audible', 't', 't')",
    ))
    .await
    .unwrap();
    restore_backup(&db, files.path(), &outcome.manifest.id, &restore_ok())
        .await
        .unwrap();
    assert_eq!(count(&db, "SELECT COUNT(*) FROM accounts").await, 1);
}

#[tokio::test]
async fn second_recovery_point_reuses_unchanged_objects() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
         VALUES ('keep', 'us', 'audible', 't', 't')",
    ))
    .await
    .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let first = backup_library(
        &db,
        &backup_req(files.path(), state.clone(), BackupReason::Manual),
    )
    .await
    .unwrap()
    .unwrap();
    let second = backup_library(&db, &backup_req(files.path(), state, BackupReason::Manual))
        .await
        .unwrap()
        .unwrap();
    let shared: BTreeSet<_> = first
        .manifest
        .referenced_objects()
        .intersection(&second.manifest.referenced_objects())
        .cloned()
        .collect();
    assert!(
        !shared.is_empty(),
        "unchanged canonical content must reuse objects"
    );
    let repo = BackupRepository::open(files.path()).unwrap();
    repo.delete_manifest(&first.manifest.id).unwrap();
    verify_recovery_point(&repo, &second.manifest.id).unwrap();
    restore_backup(&db, files.path(), &second.manifest.id, &restore_ok())
        .await
        .unwrap();
}

#[tokio::test]
async fn gc_retains_live_objects_and_drops_orphans() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let first = backup_library(
        &db,
        &backup_req(files.path(), state.clone(), BackupReason::PreMigrate),
    )
    .await
    .unwrap()
    .unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
         VALUES ('later', 'us', 'audible', 't', 't')",
    ))
    .await
    .unwrap();
    let second = backup_library(&db, &backup_req(files.path(), state, BackupReason::Manual))
        .await
        .unwrap()
        .unwrap();
    let repo = BackupRepository::open(files.path()).unwrap();
    repo.delete_manifest(&first.manifest.id).unwrap();
    let deleted = repo.gc_unreferenced_objects().unwrap();
    assert!(
        deleted > 0
            || first
                .manifest
                .referenced_objects()
                .is_subset(&second.manifest.referenced_objects())
    );
    verify_recovery_point(&repo, &second.manifest.id).unwrap();
}

#[tokio::test]
async fn gc_fails_closed_when_published_manifest_is_unreadable() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let first = backup_library(
        &db,
        &backup_req(files.path(), state.clone(), BackupReason::PreMigrate),
    )
    .await
    .unwrap()
    .unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
         VALUES ('later', 'us', 'audible', 't', 't')",
    ))
    .await
    .unwrap();
    let second = backup_library(&db, &backup_req(files.path(), state, BackupReason::Manual))
        .await
        .unwrap()
        .unwrap();
    let repo = BackupRepository::open(files.path()).unwrap();
    let manifest_path = files
        .path()
        .join(BACKUPS_DIR)
        .join("manifests")
        .join(format!("{}.json", first.manifest.id));
    std::fs::write(&manifest_path, b"{not-a-manifest").unwrap();
    let listed = repo.list_manifests();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, second.manifest.id);
    let err = repo.gc_unreferenced_objects().unwrap_err();
    assert!(
        err.to_string().contains("digest")
            || err.to_string().contains("json")
            || err.to_string().contains("manifest"),
        "{err}"
    );
    let mut retained = first.manifest.referenced_objects();
    retained.extend(second.manifest.referenced_objects());
    for digest in retained {
        let path = files
            .path()
            .join(BACKUPS_DIR)
            .join("objects")
            .join(&digest[..2])
            .join(&digest[2..]);
        assert!(
            path.is_file(),
            "GC must retain object {digest} when a published manifest is unreadable"
        );
    }
    verify_recovery_point(&repo, &second.manifest.id).unwrap();
}

#[tokio::test]
async fn capture_orders_nulls_first_with_declared_tiebreakers() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&db).await;
    apply_admitted_sql(
        &db,
        &["CREATE TABLE items (k TEXT UNIQUE, extra TEXT)"],
        CanonicalRestoreKind::PluginBinding,
    )
    .await
    .unwrap();
    for (k, extra) in [(Some("m"), Some("b")), (None, Some("z")), (None, Some("a"))] {
        db.execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO items (k, extra) VALUES (?, ?)",
            [
                bookclerk_db_exec::db_value_to_sea(&match k {
                    Some(s) => DbValue::Text(s.into()),
                    None => DbValue::Null(DbType::Text),
                }),
                bookclerk_db_exec::db_value_to_sea(&match extra {
                    Some(s) => DbValue::Text(s.into()),
                    None => DbValue::Null(DbType::Text),
                }),
            ],
        ))
        .await
        .unwrap();
    }
    let repo = BackupRepository::open(files.path()).unwrap();
    let unit = capture::capture_plugin_unit(
        &db,
        &repo,
        &CanonicalExportOpts::default(),
        "demo",
        "items",
        "sqlite",
    )
    .await
    .unwrap();
    let meta = unit.tables.iter().find(|t| t.name == "items").unwrap();
    let mut rows = Vec::new();
    for digest in &meta.chunks {
        let CanonicalObject::TableChunk { rows: chunk, .. } = repo.get_object(digest).unwrap()
        else {
            panic!("expected table chunk");
        };
        rows.extend(chunk);
    }
    assert_eq!(
        rows,
        vec![
            vec![DbValue::Null(DbType::Text), DbValue::Text("a".into())],
            vec![DbValue::Null(DbType::Text), DbValue::Text("z".into())],
            vec![DbValue::Text("m".into()), DbValue::Text("b".into())],
        ]
    );
}

#[tokio::test]
async fn missing_and_corrupt_objects_fail_verify() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let outcome = backup_library(&db, &backup_req(files.path(), state, BackupReason::Manual))
        .await
        .unwrap()
        .unwrap();
    let repo = BackupRepository::open(files.path()).unwrap();
    let digest = outcome.manifest.units[0].schema_object.clone();
    let path = files
        .path()
        .join(BACKUPS_DIR)
        .join("objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"not-a-bcko-object").unwrap();
    let err = verify_recovery_point(&repo, &outcome.manifest.id).unwrap_err();
    assert!(
        err.to_string().contains("corrupt")
            || err.to_string().contains("magic")
            || err.to_string().contains("digest"),
        "{err}"
    );
    std::fs::write(&path, original).unwrap();
    std::fs::remove_file(&path).unwrap();
    let err = verify_recovery_point(&repo, &outcome.manifest.id).unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
}

#[tokio::test]
async fn retention_never_deletes_manual_backups() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    backup_library(
        &db,
        &backup_req(files.path(), state.clone(), BackupReason::Manual),
    )
    .await
    .unwrap();
    for _ in 0..BACKUP_RETENTION + 2 {
        backup_library(
            &db,
            &backup_req(files.path(), state.clone(), BackupReason::PreMigrate),
        )
        .await
        .unwrap();
    }
    let listed = list_backups(files.path()).unwrap();
    let manuals = listed
        .iter()
        .filter(|e| e.manifest.reason == BackupReason::Manual)
        .count();
    assert_eq!(manuals, 1);
    let autos = listed
        .iter()
        .filter(|e| e.manifest.reason == BackupReason::PreMigrate)
        .count();
    assert!(autos <= BACKUP_RETENTION, "{autos}");
}

#[tokio::test]
async fn missing_consistent_backup_read_fails_closed() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let mut req = backup_req(files.path(), state, BackupReason::Manual);
    req.consistent_backup_read = false;
    let err = backup_library(&db, &req).await.unwrap_err();
    assert!(err.to_string().contains("consistentBackupRead"), "{err}");
}

#[tokio::test]
async fn restore_without_atomic_unit_restore_fails_before_drop() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
         VALUES ('keep', 'us', 'audible', 't', 't')",
    ))
    .await
    .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let outcome = backup_library(&db, &backup_req(files.path(), state, BackupReason::Manual))
        .await
        .unwrap()
        .unwrap();
    let err = restore_backup(
        &db,
        files.path(),
        &outcome.manifest.id,
        &restore_without_atomic(),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("atomicUnitRestore"), "{err}");
    assert_eq!(count(&db, "SELECT COUNT(*) FROM accounts").await, 1);
}

#[tokio::test]
async fn paged_table_larger_than_max_result_rows_round_trips() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&db).await;
    apply_admitted_sql(
        &db,
        &["CREATE TABLE items (id INTEGER PRIMARY KEY, body TEXT NOT NULL)"],
        CanonicalRestoreKind::PluginBinding,
    )
    .await
    .unwrap();
    for i in 0..25 {
        db.execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!("INSERT INTO items (id, body) VALUES ({i}, 'row-{i}')"),
        ))
        .await
        .unwrap();
    }
    let repo = BackupRepository::open(files.path()).unwrap();
    let opts = CanonicalExportOpts {
        max_result_rows: 7,
        chunk_target_bytes: 80,
        ..Default::default()
    };
    let unit = capture::capture_plugin_unit(&db, &repo, &opts, "demo", "items", "sqlite")
        .await
        .unwrap();
    let chunks: usize = unit.tables.iter().map(|t| t.chunks.len()).sum();
    assert!(chunks > 1, "expected multiple chunks, got {chunks}");
    let dest = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&dest).await;
    restore_backup_unit(
        &dest,
        &repo,
        &unit,
        CanonicalRestoreKind::PluginBinding,
        &restore_ok(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(count(&dest, "SELECT COUNT(*) FROM items").await, 25);
}

fn encoded_blob_page_bytes(n: usize, payload: &[u8]) -> usize {
    let cols = vec![
        DbColumn {
            name: "id".into(),
            db_type: DbType::Int64,
        },
        DbColumn {
            name: "blob".into(),
            db_type: DbType::Bytes,
        },
    ];
    let rows = (0..n)
        .map(|i| DbRow {
            values: vec![
                DbValue::Int64(i64::try_from(i).unwrap()),
                DbValue::Bytes(payload.to_vec()),
            ],
        })
        .collect();
    let stmt = StatementResult::from_rows(cols, rows).unwrap();
    encoded_statement_result_bytes(&stmt).unwrap().len()
}

#[tokio::test]
async fn capture_pages_when_row_count_fits_but_byte_budget_does_not() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&db).await;
    apply_admitted_sql(
        &db,
        &["CREATE TABLE blobs (id INTEGER PRIMARY KEY, blob BLOB NOT NULL)"],
        CanonicalRestoreKind::PluginBinding,
    )
    .await
    .unwrap();
    let payload = vec![0xABu8; 96];
    for i in 0..8 {
        db.execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO blobs (id, blob) VALUES (?, ?)",
            [
                bookclerk_db_exec::db_value_to_sea(&DbValue::Int64(i)),
                bookclerk_db_exec::db_value_to_sea(&DbValue::Bytes(payload.clone())),
            ],
        ))
        .await
        .unwrap();
    }
    let one = encoded_blob_page_bytes(1, &payload);
    let many = encoded_blob_page_bytes(8, &payload);
    assert!(
        many > one,
        "multi-row encoded page ({many}) must exceed one row ({one})"
    );
    let budget = u32::try_from(one + (many - one) / 2).unwrap();
    assert!(
        usize::try_from(budget).unwrap() > one,
        "budget {budget} must still fit a single row ({one})"
    );
    let repo = BackupRepository::open(files.path()).unwrap();
    let opts = CanonicalExportOpts {
        max_result_rows: 100,
        max_result_bytes: budget,
        max_atomic_result_bytes: budget,
        ..Default::default()
    };
    let unit = capture::capture_plugin_unit(&db, &repo, &opts, "demo", "blobs", "sqlite")
        .await
        .unwrap();
    let dest = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&dest).await;
    restore_backup_unit(
        &dest,
        &repo,
        &unit,
        CanonicalRestoreKind::PluginBinding,
        &restore_ok(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(count(&dest, "SELECT COUNT(*) FROM blobs").await, 8);
}

#[tokio::test]
async fn capture_fails_closed_when_schema_state_changes_inside_txn() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE schema_migrations SET checksum = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
         WHERE state = 'unreleased'",
    ))
    .await
    .unwrap();
    let repo = BackupRepository::open(files.path()).unwrap();
    let err = capture::capture_library_unit(
        &db,
        &repo,
        &state,
        &CanonicalExportOpts::default(),
        "sqlite",
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("schema state changed"), "{err}");
}

#[tokio::test]
async fn plugin_round_trip_preserves_version_and_does_not_migrate() {
    let src = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&src).await;
    apply_admitted_sql(
        &src,
        &[
            "CREATE TABLE z_parent (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE)",
            "CREATE TABLE a_child (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                parent_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                blob BLOB,
                flag BOOLEAN,
                rating REAL,
                note TEXT,
                FOREIGN KEY (parent_id) REFERENCES z_parent (id)
            )",
            "CREATE TABLE plugin_meta (schema_version INTEGER NOT NULL)",
            "CREATE UNIQUE INDEX idx_child_title ON a_child (title)",
        ],
        CanonicalRestoreKind::PluginBinding,
    )
    .await
    .unwrap();
    src.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO z_parent (id, name) VALUES (42, 'ann')",
    ))
    .await
    .unwrap();
    src.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO a_child (id, parent_id, title, blob, flag, rating, note) VALUES \
         (7, 42, '🦀', X'0001ff', 1, 1.5, NULL)",
    ))
    .await
    .unwrap();
    src.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO plugin_meta (schema_version) VALUES (7)",
    ))
    .await
    .unwrap();

    let files = tempfile::tempdir().unwrap();
    let repo = BackupRepository::open(files.path()).unwrap();
    let unit = capture::capture_plugin_unit(
        &src,
        &repo,
        &CanonicalExportOpts::default(),
        "demo",
        "notes",
        "sqlite",
    )
    .await
    .unwrap();
    assert!(unit.tables.iter().all(|t| t.name != "accounts"));

    let dest = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&dest).await;
    restore_backup_unit(
        &dest,
        &repo,
        &unit,
        CanonicalRestoreKind::PluginBinding,
        &restore_ok(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        count(&dest, "SELECT schema_version FROM plugin_meta").await,
        7
    );
    dest.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO z_parent (name) VALUES ('next')",
    ))
    .await
    .unwrap();
    let next = count(&dest, "SELECT id FROM z_parent WHERE name = 'next'").await;
    assert!(next > 42, "next id {next} must be beyond 42");
    dest.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE plugin_meta SET schema_version = 8",
    ))
    .await
    .unwrap();
    assert_eq!(
        count(&dest, "SELECT schema_version FROM plugin_meta").await,
        8
    );
}

#[tokio::test]
async fn library_only_restore_preserves_plugin_registry() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    LibraryStore::from_connection(db.clone())
        .record_plugin_database("demoplug", "notes", "sqlite", "/tmp/live.db")
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let outcome = backup_library(&db, &backup_req(files.path(), state, BackupReason::Manual))
        .await
        .unwrap()
        .unwrap();
    assert!(!outcome.manifest.include_plugin_databases);
    restore_backup(&db, files.path(), &outcome.manifest.id, &restore_ok())
        .await
        .unwrap();
    let rows = LibraryStore::from_connection(db.clone())
        .list_plugin_databases(None)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].unit_ref, "/tmp/live.db");
}

#[tokio::test]
async fn include_plugin_databases_fails_closed_without_prepared_unit() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    LibraryStore::from_connection(db.clone())
        .record_plugin_database("demoplug", "notes", "sqlite", "/tmp/unused.db")
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let mut req = backup_req(files.path(), state, BackupReason::Manual);
    req.include_plugin_databases = true;
    let err = backup_library(&db, &req).await.unwrap_err();
    assert!(
        err.to_string().contains("demoplug/notes") && err.to_string().contains("fails closed"),
        "{err}"
    );
}

#[tokio::test]
async fn include_plugin_databases_captures_and_restores_plugin_unit() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let plugin = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&plugin).await;
    apply_admitted_sql(
        &plugin,
        &["CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT)"],
        CanonicalRestoreKind::PluginBinding,
    )
    .await
    .unwrap();
    plugin
        .execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO notes (id, body) VALUES (3, 'hi')",
        ))
        .await
        .unwrap();
    LibraryStore::from_connection(db.clone())
        .record_plugin_database("demoplug", "notes", "sqlite", "memory")
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let mut req = backup_req(files.path(), state, BackupReason::Manual);
    req.include_plugin_databases = true;
    req.plugin_units = vec![prepared_plugin("demoplug", "notes", plugin.clone())];
    let outcome = backup_library(&db, &req).await.unwrap().unwrap();
    assert_eq!(outcome.manifest.units.len(), 2);

    plugin
        .execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO notes (id, body) VALUES (9, 'extra')",
        ))
        .await
        .unwrap();
    let plan = restore_backup(&db, files.path(), &outcome.manifest.id, &restore_ok())
        .await
        .unwrap();
    assert_eq!(plan.plugin_units.len(), 1);
    let after_library = LibraryStore::from_connection(db.clone())
        .list_plugin_databases(None)
        .await
        .unwrap();
    assert!(
        after_library.is_empty(),
        "included plugin DBs rebuild the registry; leftover source unit_ref must not remain"
    );
    LibraryStore::from_connection(db.clone())
        .rebind_plugin_database("demoplug", "notes", "sqlite", "/tmp/target.db")
        .await
        .unwrap();
    let rebound = LibraryStore::from_connection(db.clone())
        .get_plugin_database("demoplug", "notes")
        .await
        .unwrap()
        .expect("rebound row");
    assert_eq!(rebound.unit_ref, "/tmp/target.db");
    assert_eq!(rebound.backend_kind, "sqlite");
    let repo = BackupRepository::open(files.path()).unwrap();
    restore_backup_unit(
        &plugin,
        &repo,
        &plan.plugin_units[0],
        CanonicalRestoreKind::PluginBinding,
        &restore_ok(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(count(&plugin, "SELECT COUNT(*) FROM notes").await, 1);
}

#[tokio::test]
async fn include_plugin_databases_captures_two_bindings() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let notes = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&notes).await;
    apply_admitted_sql(
        &notes,
        &["CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT)"],
        CanonicalRestoreKind::PluginBinding,
    )
    .await
    .unwrap();
    let cache = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&cache).await;
    apply_admitted_sql(
        &cache,
        &["CREATE TABLE cache (id INTEGER PRIMARY KEY, body TEXT)"],
        CanonicalRestoreKind::PluginBinding,
    )
    .await
    .unwrap();
    let store = LibraryStore::from_connection(db.clone());
    store
        .record_plugin_database("demoplug", "notes", "sqlite", "memory-notes")
        .await
        .unwrap();
    store
        .record_plugin_database("demoplug", "cache", "sqlite", "memory-cache")
        .await
        .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let mut req = backup_req(files.path(), state, BackupReason::Manual);
    req.include_plugin_databases = true;
    req.plugin_units = vec![
        prepared_plugin("demoplug", "notes", notes),
        prepared_plugin("demoplug", "cache", cache),
    ];
    let outcome = backup_library(&db, &req).await.unwrap().unwrap();
    assert_eq!(outcome.manifest.plugin_units().len(), 2);
    let repo = BackupRepository::open(files.path()).unwrap();
    let validated = verify_recovery_point(&repo, &outcome.manifest.id).unwrap();
    assert_eq!(validated.plugin_units.len(), 2);
}

#[tokio::test]
async fn quoted_unicode_text_round_trips() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO accounts (account_id, marketplace, source, label, created_at, updated_at) \
         VALUES ('a1', 'us', 'audible', 'it''s \"quoted\" and 🦀', 't', 't')",
    ))
    .await
    .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let outcome = backup_library(&db, &backup_req(files.path(), state, BackupReason::Manual))
        .await
        .unwrap()
        .unwrap();
    restore_backup(&db, files.path(), &outcome.manifest.id, &restore_ok())
        .await
        .unwrap();
    let rows = db
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT label FROM accounts",
        ))
        .await
        .unwrap();
    let label = rows[0]
        .try_get::<Option<String>>("", "label")
        .ok()
        .flatten();
    assert_eq!(label.as_deref(), Some("it's \"quoted\" and 🦀"));
}

#[tokio::test]
async fn missing_host_table_aborts_backup() {
    let files = tempfile::tempdir().unwrap();
    let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "DROP TABLE books",
    ))
    .await
    .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let err = backup_library(&db, &backup_req(files.path(), state, BackupReason::Manual))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("cannot query") || err.to_string().contains("books"),
        "{err}"
    );
}

#[test]
fn extract_rejects_parent_dir() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("escape.tar.gz");
    {
        let file = std::fs::File::create(&archive).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(enc);
        let mut header = tar::Header::new_old();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_path("placeholder.txt").unwrap();
        header.set_size(4);
        {
            let bytes = header.as_mut_bytes();
            let name = b"../escape.txt\0";
            bytes[..name.len()].copy_from_slice(name);
            for b in &mut bytes[name.len()..100] {
                *b = 0;
            }
        }
        header.set_cksum();
        builder.append(&header, b"evil".as_slice()).unwrap();
        builder.finish().unwrap();
    }
    let out = dir.path().join("out");
    let err = extract_backup_archive(&archive, &out).unwrap_err();
    assert!(err.to_string().contains("escapes"), "{err}");
}

#[test]
fn extract_rejects_non_regular_entries() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("link.tar.gz");
    {
        let file = std::fs::File::create(&archive).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_path("ok.txt").unwrap();
        header.set_size(0);
        header.set_cksum();
        builder.append(&header, &[] as &[u8]).unwrap();
        builder.finish().unwrap();
    }
    let out = dir.path().join("out");
    let err = extract_backup_archive(&archive, &out).unwrap_err();
    assert!(err.to_string().contains("not allowed"), "{err}");
}

fn postgres_url() -> Option<String> {
    let url = std::env::var("BOOKCLERK_TEST_POSTGRES_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    if url.is_some() {
        return url;
    }
    assert!(
        std::env::var("BOOKCLERK_REQUIRE_POSTGRES_TESTS")
            .ok()
            .as_deref()
            != Some("1"),
        "BOOKCLERK_TEST_POSTGRES_URL is required when BOOKCLERK_REQUIRE_POSTGRES_TESTS=1"
    );
    None
}

async fn postgres_throwaway() -> Option<(DatabaseConnection, String)> {
    let url = postgres_url()?;
    let db_name = format!("bck_{}", uuid::Uuid::new_v4().as_simple());
    let admin = sea_orm::Database::connect(url.as_str())
        .await
        .unwrap_or_else(|err| panic!("connect BOOKCLERK_TEST_POSTGRES_URL: {err}"));
    let backend = admin.get_database_backend();
    admin
        .execute_raw(Statement::from_string(
            backend,
            format!("CREATE DATABASE {db_name}"),
        ))
        .await
        .unwrap_or_else(|err| panic!("CREATE DATABASE {db_name}: {err}"));
    let (base, query) = match url.split_once('?') {
        Some((base, q)) => (base, Some(q)),
        None => (url.as_str(), None),
    };
    let trimmed = base.trim_end_matches('/');
    let slash = trimmed
        .rfind('/')
        .unwrap_or_else(|| panic!("BOOKCLERK_TEST_POSTGRES_URL has no database path: {url}"));
    let db_url = match query {
        Some(q) => format!("{}/{db_name}?{q}", &trimmed[..slash]),
        None => format!("{}/{db_name}", &trimmed[..slash]),
    };
    let db = sea_orm::Database::connect(&db_url)
        .await
        .unwrap_or_else(|err| panic!("connect throwaway {db_name}: {err}"));
    Some((db, db_name))
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_library_backup_round_trip() {
    let Some((db, _)) = postgres_throwaway().await else {
        return;
    };
    apply_host_schema(&db, HostSchemaKind::RowMarker)
        .await
        .unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Postgres,
        "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
         VALUES ('keep', 'us', 'audible', 't', 't')",
    ))
    .await
    .unwrap();
    let state = current_schema_state(&db, HostSchemaKind::RowMarker)
        .await
        .unwrap();
    let files = tempfile::tempdir().unwrap();
    let mut req = backup_req(files.path(), state, BackupReason::Manual);
    req.backend_at_capture = "postgres".into();
    let outcome = backup_library(&db, &req).await.unwrap().unwrap();
    db.execute_raw(Statement::from_string(
        DbBackend::Postgres,
        "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
         VALUES ('extra', 'us', 'audible', 't', 't')",
    ))
    .await
    .unwrap();
    restore_backup(&db, files.path(), &outcome.manifest.id, &restore_ok())
        .await
        .unwrap();
    let n: i64 = db
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT COUNT(*) FROM accounts",
        ))
        .await
        .unwrap()
        .first()
        .and_then(|row| row.try_get_by_index::<i64>(0).ok())
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_capture_orders_nulls_first_with_declared_tiebreakers() {
    let Some((db, _)) = postgres_throwaway().await else {
        return;
    };
    apply_admitted_sql(
        &db,
        &["CREATE TABLE items (k TEXT UNIQUE, extra TEXT)"],
        CanonicalRestoreKind::PluginBinding,
    )
    .await
    .unwrap();
    for sql in [
        "INSERT INTO items (k, extra) VALUES (NULL, 'z')",
        "INSERT INTO items (k, extra) VALUES (NULL, 'a')",
        "INSERT INTO items (k, extra) VALUES ('m', 'b')",
    ] {
        db.execute_raw(Statement::from_string(DbBackend::Postgres, sql))
            .await
            .unwrap();
    }
    let files = tempfile::tempdir().unwrap();
    let repo = BackupRepository::open(files.path()).unwrap();
    let unit = capture::capture_plugin_unit(
        &db,
        &repo,
        &CanonicalExportOpts::default(),
        "demo",
        "items",
        "postgres",
    )
    .await
    .unwrap();
    let meta = unit.tables.iter().find(|t| t.name == "items").unwrap();
    let mut rows = Vec::new();
    for digest in &meta.chunks {
        let CanonicalObject::TableChunk { rows: chunk, .. } = repo.get_object(digest).unwrap()
        else {
            panic!("expected table chunk");
        };
        rows.extend(chunk);
    }
    assert_eq!(
        rows,
        vec![
            vec![DbValue::Null(DbType::Text), DbValue::Text("a".into())],
            vec![DbValue::Null(DbType::Text), DbValue::Text("z".into())],
            vec![DbValue::Text("m".into()), DbValue::Text("b".into())],
        ]
    );
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_library_restores_to_sqlite() {
    let Some((pg, _)) = postgres_throwaway().await else {
        return;
    };
    apply_host_schema(&pg, HostSchemaKind::RowMarker)
        .await
        .unwrap();
    pg.execute_raw(Statement::from_string(
        DbBackend::Postgres,
        "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
         VALUES ('keep', 'us', 'audible', 't', 't')",
    ))
    .await
    .unwrap();
    let state = current_schema_state(&pg, HostSchemaKind::RowMarker)
        .await
        .unwrap();
    let files = tempfile::tempdir().unwrap();
    let mut req = backup_req(files.path(), state, BackupReason::Manual);
    req.backend_at_capture = "postgres".into();
    let outcome = backup_library(&pg, &req).await.unwrap().unwrap();
    let sqlite = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&sqlite, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    restore_backup(&sqlite, files.path(), &outcome.manifest.id, &restore_ok())
        .await
        .unwrap();
    assert_eq!(
        count(
            &sqlite,
            "SELECT COUNT(*) FROM accounts WHERE account_id = 'keep'"
        )
        .await,
        1
    );
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_sqlite_library_restores_to_postgres() {
    let Some((pg, _)) = postgres_throwaway().await else {
        return;
    };
    let sqlite = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_host_schema(&sqlite, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    sqlite
        .execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO accounts (account_id, marketplace, source, created_at, updated_at) \
             VALUES ('keep', 'us', 'audible', 't', 't')",
        ))
        .await
        .unwrap();
    let state = current_schema_state(&sqlite, HostSchemaKind::PragmaMarker)
        .await
        .unwrap();
    let files = tempfile::tempdir().unwrap();
    let outcome = backup_library(
        &sqlite,
        &backup_req(files.path(), state, BackupReason::Manual),
    )
    .await
    .unwrap()
    .unwrap();
    apply_host_schema(&pg, HostSchemaKind::RowMarker)
        .await
        .unwrap();
    restore_backup(&pg, files.path(), &outcome.manifest.id, &restore_ok())
        .await
        .unwrap();
    let n: i64 = pg
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT COUNT(*) FROM accounts WHERE account_id = 'keep'",
        ))
        .await
        .unwrap()
        .first()
        .and_then(|row| row.try_get_by_index::<i64>(0).ok())
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
#[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
async fn postgres_sqlite_plugin_restores_to_postgres() {
    let Some((pg, _)) = postgres_throwaway().await else {
        return;
    };
    let sqlite = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
        .await
        .unwrap();
    apply_bootstrap(&sqlite).await;
    apply_admitted_sql(
        &sqlite,
        &[
            "CREATE TABLE z_parent (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
            "CREATE TABLE a_child (id INTEGER PRIMARY KEY AUTOINCREMENT, parent_id INTEGER NOT NULL, FOREIGN KEY (parent_id) REFERENCES z_parent (id))",
            "CREATE TABLE plugin_meta (schema_version INTEGER NOT NULL)",
        ],
        CanonicalRestoreKind::PluginBinding,
    )
    .await
    .unwrap();
    sqlite
        .execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO z_parent (id, name) VALUES (1, 'p')",
        ))
        .await
        .unwrap();
    sqlite
        .execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO a_child (id, parent_id) VALUES (2, 1)",
        ))
        .await
        .unwrap();
    sqlite
        .execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO plugin_meta (schema_version) VALUES (7)",
        ))
        .await
        .unwrap();
    let files = tempfile::tempdir().unwrap();
    let repo = BackupRepository::open(files.path()).unwrap();
    let unit = capture::capture_plugin_unit(
        &sqlite,
        &repo,
        &CanonicalExportOpts::default(),
        "demo",
        "notes",
        "sqlite",
    )
    .await
    .unwrap();
    restore_backup_unit(
        &pg,
        &repo,
        &unit,
        CanonicalRestoreKind::PluginBinding,
        &restore_ok(),
        false,
    )
    .await
    .unwrap();
    let version: i64 = pg
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT schema_version FROM plugin_meta",
        ))
        .await
        .unwrap()
        .first()
        .and_then(|row| row.try_get_by_index::<i64>(0).ok())
        .unwrap();
    assert_eq!(version, 7);
}

#[test]
fn d1_does_not_advertise_backup_capabilities() {
    let d1 = bookclerk_plugin_abi::DbCapabilities::advertised_d1();
    assert!(!d1.supports_consistent_backup_read());
    assert!(!d1.supports_atomic_unit_restore());
    let sqlite = bookclerk_plugin_abi::DbCapabilities::advertised_sqlite();
    assert!(sqlite.supports_consistent_backup_read());
    assert!(sqlite.supports_atomic_unit_restore());
    let pg = bookclerk_plugin_abi::DbCapabilities::advertised_postgres();
    assert!(pg.supports_consistent_backup_read());
    assert!(pg.supports_atomic_unit_restore());
}
