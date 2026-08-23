//! Host-owned schema application after a database guest connects.
//!
//! Guests open a connection and ping. The host reads the current version and
//! applies each remaining migration as **one** SQL transaction (DDL + version
//! marker last). D1 uses one HTTP `{ "batch": [...] }` per version.

use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use bookclerk_plugin_abi::{DbAtomicPlan, DbConnectResult, DbPlanStatement, DbPlanStatementKind};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::error::{LibraryError, Result};
use crate::migrations::{
    migration_sql, migration_sql_d1, migration_sql_d1_post_v27, migration_sql_postgres,
    migration_v27_d1_batch, migration_v27_schema_version,
};
use crate::sql_plan::execute_statements_on;

/// Which versioning table / dialect the host should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSchemaKind {
    /// Interactive SQLite (`PRAGMA user_version`, native transactions).
    SqliteFile,
    /// PostgreSQL (`schema_migrations` + Postgres DDL).
    Postgres,
    /// Non-interactive SQLite-family atomic-batch (Cloudflare D1).
    D1,
}

impl HostSchemaKind {
    /// Selects a schema plan from the loaded database plugin id.
    ///
    /// Adapter identity is the plugin the host loaded (`sqlite`, `d1`,
    /// `postgres`, plus documented aliases). Advertised capability flags are
    /// verified separately with [`Self::advertised_flags_match`].
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Other`] when `plugin_id` is not a first-party
    /// database plugin.
    pub fn from_plugin_id(plugin_id: &str) -> Result<Self> {
        match plugin_id.trim().to_ascii_lowercase().as_str() {
            "sqlite" | "local" => Ok(Self::SqliteFile),
            "d1" | "cloudflare-d1" | "cloudflare_d1" => Ok(Self::D1),
            "postgres" | "postgresql" | "pg" => Ok(Self::Postgres),
            other => Err(LibraryError::Other(anyhow::anyhow!(
                "unknown database plugin `{other}`"
            ))),
        }
    }

    /// Checks that advertised schema flags match this plugin kind.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Other`] when the guest advertised a different
    /// versioning scheme than this kind requires.
    pub fn advertised_flags_match(self, caps: &DbConnectResult) -> Result<()> {
        let ok = match self {
            Self::SqliteFile => caps.pragma_user_version && !caps.atomic_schema_batch,
            Self::D1 => caps.schema_migrations && caps.atomic_schema_batch,
            Self::Postgres => {
                caps.schema_migrations && !caps.atomic_schema_batch && !caps.pragma_user_version
            }
        };
        if ok {
            Ok(())
        } else {
            Err(LibraryError::Other(anyhow::anyhow!(
                "database plugin advertised schema flags do not match {:?}",
                self
            )))
        }
    }
}

/// Applies pending host-authored DDL. D1 V27 is skipped (use
/// [`apply_host_schema_with_batch`]).
///
/// # Errors
///
/// Returns [`LibraryError`] when a version read or DDL statement fails.
pub async fn apply_host_schema(db: &DatabaseConnection, kind: HostSchemaKind) -> Result<()> {
    match kind {
        HostSchemaKind::SqliteFile => apply_sqlite_user_version(db).await,
        HostSchemaKind::Postgres => {
            apply_schema_migrations(
                db,
                DbBackend::Postgres,
                "postgres_txn",
                migration_sql_postgres(),
            )
            .await
        }
        HostSchemaKind::D1 => {
            apply_schema_migrations(db, DbBackend::Sqlite, "sqlite_txn", migration_sql_d1())
                .await?;
            let v27 = migration_v27_schema_version();
            if !schema_version_applied(db, DbBackend::Sqlite, v27).await? {
                return Err(LibraryError::Other(anyhow::anyhow!(
                    "D1 V27 must be applied as one atomic batch (apply_host_schema_with_batch)"
                )));
            }
            apply_d1_post_v27_versions(db).await
        }
    }
}

/// Applies sqlite/postgres schema using `run_batch` (typed `executeAtomic`) for
/// each version, instead of SeaORM interactive transactions.
///
/// # Errors
///
/// Returns [`LibraryError`] when a version read, DDL statement, or batch fails.
async fn apply_host_schema_native_or_batch<F, Fut>(
    db: &DatabaseConnection,
    kind: HostSchemaKind,
    mut run_batch: F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    match kind {
        HostSchemaKind::SqliteFile => {
            apply_sqlite_user_version_with_batch(db, &mut run_batch).await
        }
        HostSchemaKind::Postgres => {
            apply_schema_migrations_with_batch(
                db,
                DbBackend::Postgres,
                migration_sql_postgres(),
                &mut run_batch,
            )
            .await
        }
        HostSchemaKind::D1 => apply_host_schema(db, kind).await,
    }
}

/// Applies pending DDL, running each D1 version (including V27) as one batch.
///
/// # Errors
///
/// Returns [`LibraryError`] when a version read, autocommit step, or batch fails.
pub async fn apply_host_schema_with_batch<F, Fut>(
    db: &DatabaseConnection,
    kind: HostSchemaKind,
    mut run_batch: F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    if kind != HostSchemaKind::D1 {
        return apply_host_schema_native_or_batch(db, kind, run_batch).await;
    }
    ensure_schema_migrations(db, DbBackend::Sqlite).await?;
    let mut applied = schema_versions_applied(db, DbBackend::Sqlite).await?;
    for (idx, schema) in migration_sql_d1().iter().enumerate() {
        let version = (idx + 1) as i64;
        let mut stmts = split_sql_statements(schema);
        stmts.push(format!(
            "INSERT INTO schema_migrations (version) VALUES ({version})"
        ));
        apply_one_d1_batch(db, version, stmts, &mut run_batch, &mut applied).await?;
    }
    let v27 = migration_v27_schema_version();
    if !applied.contains(&v27) {
        apply_one_d1_batch(
            db,
            v27,
            migration_v27_d1_batch(),
            &mut run_batch,
            &mut applied,
        )
        .await?;
    }
    let mut version = v27;
    for schema in migration_sql_d1_post_v27() {
        version += 1;
        let mut stmts = split_sql_statements(schema);
        stmts.push(format!(
            "INSERT INTO schema_migrations (version) VALUES ({version})"
        ));
        apply_one_d1_batch(db, version, stmts, &mut run_batch, &mut applied).await?;
    }
    Ok(())
}

/// Applies one D1 version batch, reloading `schema_migrations` after errors.
async fn apply_one_d1_batch<F, Fut>(
    db: &DatabaseConnection,
    version: i64,
    stmts: Vec<String>,
    run_batch: &mut F,
    applied: &mut HashSet<i64>,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let mut delay_ms = 20u64;
    let mut last_err = None;
    for attempt in 0..8 {
        if applied.contains(&version) {
            return Ok(());
        }
        match run_batch(stmts.clone()).await {
            Ok(()) => {
                applied.insert(version);
                return Ok(());
            }
            Err(err) => {
                *applied = schema_versions_applied(db, DbBackend::Sqlite).await?;
                if applied.contains(&version) {
                    return Ok(());
                }
                if attempt + 1 < 8 && is_schema_lock_err(&err) {
                    last_err = Some(err);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    delay_ms = delay_ms.saturating_mul(2).min(250);
                    continue;
                }
                return Err(err);
            }
        }
    }
    *applied = schema_versions_applied(db, DbBackend::Sqlite).await?;
    if applied.contains(&version) {
        return Ok(());
    }
    Err(last_err.expect("d1 schema version retry"))
}

/// Applies pending SQLite `PRAGMA user_version` migrations via `run_batch`.
///
/// # Errors
///
/// Returns when a version read or batch fails.
async fn apply_sqlite_user_version_with_batch<F, Fut>(
    db: &DatabaseConnection,
    run_batch: &mut F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let backend = DbBackend::Sqlite;
    exec_sql(db, backend, "PRAGMA foreign_keys = OFF").await?;
    let steps = migration_sql();
    for (idx, schema) in steps.iter().enumerate() {
        let version = (idx + 1) as i64;
        apply_one_sqlite_version_with_batch(db, backend, version, schema, run_batch).await?;
    }
    exec_sql(db, backend, "PRAGMA foreign_keys = ON").await?;
    Ok(())
}

/// Applies one SQLite schema version as a `run_batch` transaction.
///
/// # Errors
///
/// Returns when the batch fails.
async fn apply_one_sqlite_version_with_batch<F, Fut>(
    db: &DatabaseConnection,
    _backend: DbBackend,
    version: i64,
    schema: &str,
    run_batch: &mut F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let mut delay_ms = 20u64;
    let mut last_applied_err = None;
    for attempt in 0..8 {
        if version <= sqlite_user_version(db).await? {
            return Ok(());
        }
        let mut stmts = split_sql_statements(schema);
        stmts.push(format!("PRAGMA user_version = {version}"));
        match run_batch(stmts).await {
            Ok(()) => return Ok(()),
            Err(err) if is_already_applied_ddl(&err) => {
                tokio::time::sleep(Duration::from_millis(15)).await;
                last_applied_err = Some(err);
            }
            Err(err) if attempt + 1 < 8 && is_schema_lock_err(&err) => {
                last_applied_err = Some(err);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms = delay_ms.saturating_mul(2).min(250);
            }
            Err(err) => return Err(err),
        }
    }
    if version <= sqlite_user_version(db).await? {
        return Ok(());
    }
    Err(last_applied_err.expect("sqlite schema version retry"))
}

/// Applies pending `schema_migrations` versions via `run_batch`.
///
/// # Errors
///
/// Returns when a version read or batch fails.
async fn apply_schema_migrations_with_batch<F, Fut>(
    db: &DatabaseConnection,
    backend: DbBackend,
    steps: &[&str],
    run_batch: &mut F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    ensure_schema_migrations(db, backend).await?;
    for (idx, schema) in steps.iter().enumerate() {
        let version = (idx + 1) as i64;
        let mut delay_ms = 20u64;
        let mut last_err = None;
        for attempt in 0..8 {
            if schema_versions_applied(db, backend)
                .await?
                .contains(&version)
            {
                break;
            }
            let mut stmts = split_sql_statements(schema);
            stmts.push(format!(
                "INSERT INTO schema_migrations (version) VALUES ({version})"
            ));
            match run_batch(stmts).await {
                Ok(()) => break,
                Err(err) if is_already_applied_ddl(&err) => {
                    tokio::time::sleep(Duration::from_millis(15)).await;
                    last_err = Some(err);
                }
                Err(err) if attempt + 1 < 8 && is_schema_lock_err(&err) => {
                    last_err = Some(err);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    delay_ms = delay_ms.saturating_mul(2).min(250);
                }
                Err(err) => return Err(err),
            }
        }
        if !schema_versions_applied(db, backend)
            .await?
            .contains(&version)
        {
            if let Some(err) = last_err {
                return Err(err);
            }
        }
    }
    Ok(())
}

/// Applies remaining `PRAGMA user_version` steps (file SQLite).
async fn apply_sqlite_user_version(db: &DatabaseConnection) -> Result<()> {
    let backend = DbBackend::Sqlite;
    exec_sql(db, backend, "PRAGMA foreign_keys = OFF").await?;
    let steps = migration_sql();
    for (idx, schema) in steps.iter().enumerate() {
        let version = (idx + 1) as i64;
        apply_one_sqlite_version(db, backend, version, schema).await?;
    }
    exec_sql(db, backend, "PRAGMA foreign_keys = ON").await?;
    Ok(())
}

/// Applies one file-SQLite version, recovering when a peer committed DDL first.
async fn apply_one_sqlite_version(
    db: &DatabaseConnection,
    backend: DbBackend,
    version: i64,
    schema: &str,
) -> Result<()> {
    let mut delay_ms = 20u64;
    let mut last_applied_err = None;
    for attempt in 0..8 {
        if version <= sqlite_user_version(db).await? {
            return Ok(());
        }
        let mut stmts = split_sql_statements(schema);
        stmts.push(format!("PRAGMA user_version = {version}"));
        match run_atomic_ddl(
            db,
            backend,
            "sqlite_txn",
            &format!("migrate-sqlite-{version}"),
            stmts,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(err) if is_already_applied_ddl(&err) => {
                // Peer's ALTER is visible before its version marker commits.
                tokio::time::sleep(Duration::from_millis(15)).await;
                last_applied_err = Some(err);
            }
            Err(err) if attempt + 1 < 8 && is_schema_lock_err(&err) => {
                last_applied_err = Some(err);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms = delay_ms.saturating_mul(2).min(250);
            }
            Err(err) => return Err(err),
        }
    }
    if version <= sqlite_user_version(db).await? {
        return Ok(());
    }
    Err(last_applied_err.expect("sqlite schema version retry"))
}

/// Additive sqlite steps after the D1 V27 batch (versions 29+).
///
/// File-SQLite V28 is `PRAGMA user_version` 28, but D1 already recorded 28 for
/// the V27 batch, so post-V27 steps are numbered from 29.
async fn apply_d1_post_v27_versions(db: &DatabaseConnection) -> Result<()> {
    let backend = DbBackend::Sqlite;
    let mut version = migration_v27_schema_version();
    for schema in migration_sql_d1_post_v27() {
        version += 1;
        if schema_versions_applied(db, backend)
            .await?
            .contains(&version)
        {
            continue;
        }
        let mut stmts = split_sql_statements(schema);
        stmts.push(format!(
            "INSERT INTO schema_migrations (version) VALUES ({version})"
        ));
        match run_atomic_ddl_retrying(
            db,
            backend,
            "sqlite_txn",
            &format!("migrate-d1-post-{version}"),
            stmts,
        )
        .await
        {
            Ok(()) => {}
            Err(_)
                if schema_versions_applied(db, backend)
                    .await?
                    .contains(&version) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Reads `PRAGMA user_version`.
async fn sqlite_user_version(db: &DatabaseConnection) -> Result<i64> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA user_version",
        ))
        .await
        .map_err(LibraryError::Orm)?;
    let Some(row) = rows.first() else {
        return Ok(0);
    };
    if let Ok(v) = row.try_get::<i64>("", "user_version") {
        return Ok(v);
    }
    if let Ok(v) = row.try_get_by_index::<i64>(0) {
        return Ok(v);
    }
    if let Ok(v) = row.try_get_by_index::<i32>(0) {
        return Ok(i64::from(v));
    }
    Ok(0)
}

/// Applies `steps` not yet recorded in `schema_migrations` (version = index + 1).
async fn apply_schema_migrations(
    db: &DatabaseConnection,
    backend: DbBackend,
    timing: &str,
    steps: &[&str],
) -> Result<()> {
    ensure_schema_migrations(db, backend).await?;
    for (idx, schema) in steps.iter().enumerate() {
        let version = (idx + 1) as i64;
        apply_one_schema_migration(db, backend, timing, version, schema).await?;
    }
    Ok(())
}

/// Applies one `schema_migrations` version, recovering from a concurrent peer.
async fn apply_one_schema_migration(
    db: &DatabaseConnection,
    backend: DbBackend,
    timing: &str,
    version: i64,
    schema: &str,
) -> Result<()> {
    let mut delay_ms = 20u64;
    let mut last_err = None;
    for attempt in 0..8 {
        if schema_versions_applied(db, backend)
            .await?
            .contains(&version)
        {
            return Ok(());
        }
        let mut stmts = split_sql_statements(schema);
        stmts.push(format!(
            "INSERT INTO schema_migrations (version) VALUES ({version})"
        ));
        match run_atomic_ddl(
            db,
            backend,
            timing,
            &format!("migrate-{timing}-{version}"),
            stmts,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(err) if is_already_applied_ddl(&err) => {
                tokio::time::sleep(Duration::from_millis(15)).await;
                last_err = Some(err);
            }
            Err(err) if attempt + 1 < 8 && is_schema_lock_err(&err) => {
                last_err = Some(err);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms = delay_ms.saturating_mul(2).min(250);
            }
            Err(err) => return Err(err),
        }
    }
    if schema_versions_applied(db, backend)
        .await?
        .contains(&version)
    {
        return Ok(());
    }
    Err(last_err.expect("schema_migrations version retry"))
}

/// `CREATE TABLE IF NOT EXISTS schema_migrations`.
async fn ensure_schema_migrations(db: &DatabaseConnection, backend: DbBackend) -> Result<()> {
    let mut delay_ms = 20u64;
    let mut last_err = None;
    for attempt in 0..8 {
        match exec_sql(
            db,
            backend,
            "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY)",
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(err)
                if attempt + 1 < 8
                    && (is_already_applied_ddl(&err) || is_schema_lock_err(&err)) =>
            {
                last_err = Some(err);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms = delay_ms.saturating_mul(2).min(250);
            }
            Err(err) => return Err(err),
        }
    }
    match exec_sql(
        db,
        backend,
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY)",
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(_) if last_err.is_some() => {
            // Peer created the table; a follow-up SELECT in the migrator confirms it.
            Ok(())
        }
        Err(err) => Err(err),
    }
}

/// Runs `stmts` as one generic atomic execute plan (version marker last).
async fn run_atomic_ddl(
    db: &DatabaseConnection,
    backend: DbBackend,
    timing: &str,
    operation_id: &str,
    stmts: Vec<String>,
) -> Result<()> {
    if stmts.is_empty() {
        return Ok(());
    }
    let _ = backend;
    let plan = DbAtomicPlan {
        statements: stmts
            .into_iter()
            .map(|sql| DbPlanStatement::new(sql, Vec::new(), DbPlanStatementKind::Execute))
            .collect(),
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    execute_statements_on(db, &plan, operation_id, timing, 0).await?;
    Ok(())
}

/// [`run_atomic_ddl`] with retries on engine lock / deadlock.
async fn run_atomic_ddl_retrying(
    db: &DatabaseConnection,
    backend: DbBackend,
    timing: &str,
    operation_id: &str,
    stmts: Vec<String>,
) -> Result<()> {
    let mut delay_ms = 20u64;
    let mut last_lock_err = None;
    for attempt in 0..8 {
        match run_atomic_ddl(db, backend, timing, operation_id, stmts.clone()).await {
            Ok(()) => return Ok(()),
            Err(err) if attempt + 1 < 8 && is_schema_lock_err(&err) => {
                last_lock_err = Some(err);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms = delay_ms.saturating_mul(2).min(250);
            }
            Err(err) => return Err(err),
        }
    }
    Err(last_lock_err.expect("schema ddl lock retry"))
}

/// True when a concurrent migrator already applied this version's DDL.
fn is_already_applied_ddl(err: &LibraryError) -> bool {
    let s = format!("{err}\n{err:?}").to_lowercase();
    s.contains("duplicate column")
        || s.contains("already exists")
        || s.contains("duplicate key")
        || s.contains("23505")
        || s.contains("sqlite_constraint")
}

/// True when a concurrent migrator should retry this version.
fn is_schema_lock_err(err: &LibraryError) -> bool {
    let s = format!("{err}\n{err:?}");
    s.contains("SQLITE_BUSY")
        || s.contains("SQLITE_LOCKED")
        || s.contains("database is locked")
        || s.contains("begin failed")
        || s.contains("40P01")
        || s.contains("40001")
        || s.contains("55P03")
}

/// Loads `schema_migrations.version` rows.
async fn schema_versions_applied(
    db: &DatabaseConnection,
    backend: DbBackend,
) -> Result<HashSet<i64>> {
    let rows = db
        .query_all_raw(Statement::from_string(
            backend,
            "SELECT version FROM schema_migrations",
        ))
        .await
        .map_err(LibraryError::Orm)?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            row.try_get::<i64>("", "version")
                .ok()
                .or_else(|| row.try_get_by_index::<i64>(0).ok())
                .or_else(|| row.try_get::<i32>("", "version").ok().map(i64::from))
                .or_else(|| row.try_get_by_index::<i32>(0).ok().map(i64::from))
        })
        .collect())
}

/// True when `schema_migrations` already contains `version`.
async fn schema_version_applied(
    db: &DatabaseConnection,
    backend: DbBackend,
    version: i64,
) -> Result<bool> {
    let sql = if backend == DbBackend::Postgres {
        "SELECT version FROM schema_migrations WHERE version = $1"
    } else {
        "SELECT version FROM schema_migrations WHERE version = ?"
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            backend,
            sql,
            [Value::from(version)],
        ))
        .await
        .map_err(LibraryError::Orm)?;
    Ok(!rows.is_empty())
}

/// Executes one SQL string.
async fn exec_sql(db: &DatabaseConnection, backend: DbBackend, sql: &str) -> Result<()> {
    db.execute_raw(Statement::from_string(backend, sql.to_string()))
        .await
        .map_err(LibraryError::Orm)?;
    Ok(())
}

/// Splits a migration script on `;` and drops empty fragments.
fn split_sql_statements(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookclerk_plugin_abi::DbConnectResult;

    #[test]
    fn from_plugin_id_selects_kind_and_verifies_flags() {
        let mut sqlite = DbConnectResult::sqlite();
        let kind = HostSchemaKind::from_plugin_id("sqlite").unwrap();
        assert_eq!(kind, HostSchemaKind::SqliteFile);
        kind.advertised_flags_match(&sqlite).unwrap();
        sqlite.interactive_txn = false;
        sqlite.sql_family = "postgres".into();
        kind.advertised_flags_match(&sqlite).unwrap();
        assert_eq!(
            HostSchemaKind::from_plugin_id("local").unwrap(),
            HostSchemaKind::SqliteFile
        );

        let pg = HostSchemaKind::from_plugin_id("postgres").unwrap();
        assert_eq!(pg, HostSchemaKind::Postgres);
        pg.advertised_flags_match(&DbConnectResult::postgres())
            .unwrap();

        let mut d1 = DbConnectResult::d1();
        let d1_kind = HostSchemaKind::from_plugin_id("d1").unwrap();
        assert_eq!(d1_kind, HostSchemaKind::D1);
        d1_kind.advertised_flags_match(&d1).unwrap();
        d1.interactive_txn = true;
        d1.sql_family = "postgres".into();
        d1.dialect = "postgres".into();
        d1_kind.advertised_flags_match(&d1).unwrap();

        assert!(HostSchemaKind::from_plugin_id("mysql").is_err());
        let mut none = DbConnectResult::sqlite();
        none.pragma_user_version = false;
        assert!(HostSchemaKind::SqliteFile
            .advertised_flags_match(&none)
            .is_err());
    }

    #[tokio::test]
    async fn d1_post_v27_versions_follow_the_batch_marker() {
        let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
            .await
            .expect("unmigrated sqlite");
        let db_batch = db.clone();
        apply_host_schema_with_batch(&db, HostSchemaKind::D1, move |stmts| {
            let db_batch = db_batch.clone();
            async move {
                run_atomic_ddl(
                    &db_batch,
                    DbBackend::Sqlite,
                    "sqlite_txn",
                    "d1-batch",
                    stmts,
                )
                .await
            }
        })
        .await
        .expect("d1 schema");
        let applied = schema_versions_applied(&db, DbBackend::Sqlite)
            .await
            .unwrap();
        let v27 = migration_v27_schema_version();
        assert!(applied.contains(&v27), "V27 batch marker {v27}");
        let post = i64::try_from(migration_sql_d1_post_v27().len()).unwrap();
        assert!(
            applied.contains(&(v27 + post)),
            "last post-V27 version {}, applied={applied:?}",
            v27 + post
        );
        let cols = db
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(domain_events)",
            ))
            .await
            .unwrap();
        let names: Vec<String> = cols
            .iter()
            .filter_map(|row| row.try_get::<String>("", "name").ok())
            .collect();
        assert!(
            names.iter().any(|n| n == "dispatch_snapshot_json"),
            "dispatch snapshot column missing: {names:?}"
        );
        db.query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT slot_key FROM db_serialization_slots LIMIT 1",
        ))
        .await
        .expect("serialization slots table");
    }

    #[tokio::test]
    async fn sqlite_crash_before_version_marker_rolls_back_and_retries() {
        let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
            .await
            .expect("unmigrated sqlite");
        let ddl = split_sql_statements(migration_sql()[0]).len() as u32;
        crate::inject_atomic_interrupt_after(
            crate::AtomicInterruptPhase::BetweenStatements,
            crate::AtomicInterruptKind::Cancel,
            ddl,
        );
        let err = apply_host_schema(&db, HostSchemaKind::SqliteFile)
            .await
            .expect_err("interrupt before version marker");
        assert!(err.to_string().to_lowercase().contains("cancel"), "{err}");
        assert_eq!(sqlite_user_version(&db).await.unwrap(), 0);
        apply_host_schema(&db, HostSchemaKind::SqliteFile)
            .await
            .expect("retry after crash");
        assert!(sqlite_user_version(&db).await.unwrap() > 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sqlite_concurrent_apply_host_schema_both_ok() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lib.db");
        let db1 = bookclerk_plugin_database_sqlite::open(&path)
            .await
            .expect("open 1");
        let db2 = bookclerk_plugin_database_sqlite::open(&path)
            .await
            .expect("open 2");
        let (a, b) = tokio::join!(
            apply_host_schema(&db1, HostSchemaKind::SqliteFile),
            apply_host_schema(&db2, HostSchemaKind::SqliteFile),
        );
        a.expect("first apply");
        b.expect("second apply");
        assert!(sqlite_user_version(&db1).await.unwrap() > 0);
    }

    #[tokio::test]
    #[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
    async fn postgres_crash_before_version_marker_rolls_back_and_retries() {
        if std::env::var("BOOKCLERK_TEST_POSTGRES_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .is_none()
        {
            return;
        }
        let url = std::env::var("BOOKCLERK_TEST_POSTGRES_URL").unwrap();
        let db_name = format!("mig_{}", uuid::Uuid::new_v4().as_simple());
        let admin = sea_orm::Database::connect(url.as_str())
            .await
            .expect("admin");
        let backend = sea_orm::ConnectionTrait::get_database_backend(&admin);
        sea_orm::ConnectionTrait::execute_raw(
            &admin,
            sea_orm::Statement::from_string(backend, format!("CREATE DATABASE {db_name}")),
        )
        .await
        .expect("create db");
        let (base, query) = match url.split_once('?') {
            Some((base, q)) => (base, Some(q)),
            None => (url.as_str(), None),
        };
        let trimmed = base.trim_end_matches('/');
        let slash = trimmed.rfind('/').expect("url path");
        let db_url = match query {
            Some(q) => format!("{}/{db_name}?{q}", &trimmed[..slash]),
            None => format!("{}/{db_name}", &trimmed[..slash]),
        };
        let db = sea_orm::Database::connect(&db_url).await.expect("connect");
        let ddl = split_sql_statements(migration_sql_postgres()[0]).len() as u32;
        crate::inject_atomic_interrupt_after(
            crate::AtomicInterruptPhase::BetweenStatements,
            crate::AtomicInterruptKind::Cancel,
            ddl,
        );
        let err = apply_host_schema(&db, HostSchemaKind::Postgres)
            .await
            .expect_err("interrupt");
        assert!(err.to_string().to_lowercase().contains("cancel"), "{err}");
        apply_host_schema(&db, HostSchemaKind::Postgres)
            .await
            .expect("retry");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL"]
    async fn postgres_concurrent_apply_host_schema_both_ok() {
        if std::env::var("BOOKCLERK_TEST_POSTGRES_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .is_none()
        {
            return;
        }
        let url = std::env::var("BOOKCLERK_TEST_POSTGRES_URL").unwrap();
        let db_name = format!("migc_{}", uuid::Uuid::new_v4().as_simple());
        let admin = sea_orm::Database::connect(url.as_str())
            .await
            .expect("admin");
        let backend = sea_orm::ConnectionTrait::get_database_backend(&admin);
        sea_orm::ConnectionTrait::execute_raw(
            &admin,
            sea_orm::Statement::from_string(backend, format!("CREATE DATABASE {db_name}")),
        )
        .await
        .expect("create db");
        let (base, query) = match url.split_once('?') {
            Some((base, q)) => (base, Some(q)),
            None => (url.as_str(), None),
        };
        let trimmed = base.trim_end_matches('/');
        let slash = trimmed.rfind('/').expect("url path");
        let db_url = match query {
            Some(q) => format!("{}/{db_name}?{q}", &trimmed[..slash]),
            None => format!("{}/{db_name}", &trimmed[..slash]),
        };
        let db1 = sea_orm::Database::connect(&db_url).await.expect("c1");
        let db2 = sea_orm::Database::connect(&db_url).await.expect("c2");
        let (a, b) = tokio::join!(
            apply_host_schema(&db1, HostSchemaKind::Postgres),
            apply_host_schema(&db2, HostSchemaKind::Postgres),
        );
        a.expect("first apply");
        b.expect("second apply");
    }
}
