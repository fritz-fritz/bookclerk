//! Host-owned schema application after a database guest connects.
//!
//! Guests open a connection and ping. The host reads the current version and
//! applies remaining [`crate::migrations::host_migration_plan`] steps as **one**
//! atomic unit (DDL + version marker last). A database newer than
//! [`crate::migrations::SCHEMA_VERSION`] fails closed. Marker kind selects only
//! the versioning mechanic (`PRAGMA user_version`, `schema_migrations` row, or
//! one HTTP `{ "batch": [...] }` per version).

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use bookclerk_plugin_abi::DbPlanStatementKind;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::error::{LibraryError, Result};
use crate::migrations::{
    host_migration_plan, HostMigrationStep, SCHEMA_MIGRATIONS_DDL, SCHEMA_VERSION,
};
use crate::schema_snapshot::{snapshot_library, SnapshotRequest};
use crate::schema_walk::{plan_schema_walk, SchemaWalk};
use crate::sql_plan::{execute_statements_on, DbAtomicPlan, DbPlanStatement};

/// Optional in-place snapshot taken before applying ups or downs.
#[derive(Debug, Clone, Default)]
pub struct SchemaSnapshotOpts {
    /// `$BOOKCLERK_FILES_DIR` root for `snapshots/`.
    pub files_dir: PathBuf,
    /// File SQLite path for `VACUUM INTO`; omit for Postgres / D1 SQL dumps.
    pub sqlite_path: Option<PathBuf>,
    /// Copy `plugin-databases/` when true (off for automatic connect).
    pub include_plugin_databases: bool,
    /// Precomputed SQL dump (Cloudflare D1 REST export). When set, written as
    /// `library.sql` instead of a SELECT-through-connection dump.
    pub sql_dump: Option<Vec<u8>>,
}

/// Options for [`apply_host_schema_with_options`].
#[derive(Debug, Clone, Default)]
pub struct SchemaApplyOptions {
    /// When set, snapshot before applying DDL to a non-empty database.
    pub snapshot: Option<SchemaSnapshotOpts>,
}

/// Which versioning mechanic the host should use.
///
/// Flags choose **how** versions are stored and applied, not which SQL pack
/// to emit. Canonical Bookclerk SQL is [`crate::migrations::host_migration_plan`].
/// Adapters lower canonical DDL for the live connection backend at execution
/// (see [`bookclerk_db_exec::expand_host_schema_batch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSchemaKind {
    /// `PRAGMA user_version` on an interactive SQLite-family connection.
    PragmaMarker,
    /// `schema_migrations` rows without requiring atomic HTTP batches.
    RowMarker,
    /// `schema_migrations` plus one atomic batch per version (D1-style).
    AtomicBatchMarker,
}

impl HostSchemaKind {
    /// Selects a schema **apply mechanic** from typed
    /// [`bookclerk_plugin_abi::DbCapabilities`] versioning flags.
    ///
    /// Plugin identity, `dialect`, and `sqlFamily` are not consulted. SQL text
    /// is chosen from the live connection backend when applying (canonical
    /// SQLite pack, or the Postgres adapter-edge pack in `bookclerk-db-exec`),
    /// not from these flags. A conforming adapter may use any plugin id as
    /// long as it advertises exactly one of:
    ///
    /// - `pragmaUserVersion` (`PRAGMA user_version` marker)
    /// - `schemaMigrations` without `atomicSchemaBatch` (row marker)
    /// - `schemaMigrations` + `atomicSchemaBatch` (atomic batch apply)
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Other`] when the flags are missing, mixed, or
    /// contradictory.
    pub fn from_db_capabilities(caps: &bookclerk_plugin_abi::DbCapabilities) -> Result<Self> {
        let kind = if caps.pragma_user_version
            && !caps.schema_migrations
            && !caps.atomic_schema_batch
        {
            Self::PragmaMarker
        } else if caps.schema_migrations && caps.atomic_schema_batch && !caps.pragma_user_version {
            Self::AtomicBatchMarker
        } else if caps.schema_migrations && !caps.atomic_schema_batch && !caps.pragma_user_version {
            Self::RowMarker
        } else {
            return Err(LibraryError::Other(anyhow::anyhow!(
                "database guest schema flags are not a known versioning contract \
                 (pragmaUserVersion={}, schemaMigrations={}, atomicSchemaBatch={})",
                caps.pragma_user_version,
                caps.schema_migrations,
                caps.atomic_schema_batch
            )));
        };
        kind.advertised_db_capabilities_match(caps)?;
        Ok(kind)
    }

    /// Checks typed capability flags against this marker kind.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Other`] when the guest advertised a different
    /// versioning scheme than this kind requires.
    pub fn advertised_db_capabilities_match(
        self,
        caps: &bookclerk_plugin_abi::DbCapabilities,
    ) -> Result<()> {
        let ok = match self {
            Self::PragmaMarker => caps.pragma_user_version && !caps.atomic_schema_batch,
            Self::AtomicBatchMarker => caps.schema_migrations && caps.atomic_schema_batch,
            Self::RowMarker => {
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

/// Applies pending host-authored DDL from the canonical migration plan.
///
/// # Errors
///
/// Returns [`LibraryError`] when a version read or DDL statement fails, the
/// database is newer than this binary, or a frozen checksum does not match.
pub async fn apply_host_schema(db: &DatabaseConnection, kind: HostSchemaKind) -> Result<()> {
    apply_host_schema_with_options(db, kind, SchemaApplyOptions::default()).await
}

/// Applies pending host schema with optional in-place snapshots.
///
/// # Errors
///
/// Returns [`LibraryError`] when a version read, snapshot, or DDL statement fails.
pub async fn apply_host_schema_with_options(
    db: &DatabaseConnection,
    kind: HostSchemaKind,
    opts: SchemaApplyOptions,
) -> Result<()> {
    let walk = prepare_schema_change(db, kind, SCHEMA_VERSION, &opts).await?;
    apply_walk_direct(db, kind, &walk).await
}

/// Applies schema using `run_batch` (typed `executeAtomic`) for each version.
///
/// # Errors
///
/// Returns [`LibraryError`] when a version read, DDL statement, or batch fails.
pub async fn apply_host_schema_with_batch<F, Fut>(
    db: &DatabaseConnection,
    kind: HostSchemaKind,
    run_batch: F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    apply_host_schema_with_batch_opts(db, kind, SchemaApplyOptions::default(), run_batch).await
}

/// Applies schema with snapshots using `run_batch` for each version.
///
/// # Errors
///
/// Returns [`LibraryError`] when a version read, snapshot, DDL, or batch fails.
pub async fn apply_host_schema_with_batch_opts<F, Fut>(
    db: &DatabaseConnection,
    kind: HostSchemaKind,
    opts: SchemaApplyOptions,
    mut run_batch: F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let walk = prepare_schema_change(db, kind, SCHEMA_VERSION, &opts).await?;
    apply_walk_batch(db, kind, &walk, &mut run_batch).await
}

/// Migrates toward `target`, including reversible downs for CLI rollback.
///
/// # Errors
///
/// Returns [`LibraryError`] when the walk cannot start, a snapshot fails, or DDL fails.
/// A blocked downgrade still applies every reversible step and returns the walk
/// (the caller treats `blocked` / `stopped_at != target` as failure).
pub async fn migrate_host_schema_to(
    db: &DatabaseConnection,
    kind: HostSchemaKind,
    target: i64,
    opts: SchemaApplyOptions,
) -> Result<SchemaWalk> {
    let walk = prepare_schema_change(db, kind, target, &opts).await?;
    apply_walk_direct(db, kind, &walk).await?;
    Ok(walk)
}

/// Timing label for schema migration transactions on `backend`.
fn schema_migration_timing(backend: DbBackend) -> &'static str {
    if backend == DbBackend::Postgres {
        "postgres_txn"
    } else {
        "sqlite_txn"
    }
}

/// Reads the current schema version, verifies checksums, snapshots, and plans a walk.
async fn prepare_schema_change(
    db: &DatabaseConnection,
    kind: HostSchemaKind,
    target: i64,
    opts: &SchemaApplyOptions,
) -> Result<SchemaWalk> {
    let backend = db.get_database_backend();
    ensure_schema_migrations(db, backend).await?;
    let current = current_schema_version(db, kind).await?;
    let plan = host_migration_plan();
    verify_applied_checksums(db, backend, &plan, current).await?;
    let walk = plan_schema_walk(&plan, current, target)?;
    if !walk.is_noop() {
        if let Some(snap) = opts.snapshot.as_ref() {
            let req = SnapshotRequest {
                files_dir: snap.files_dir.clone(),
                from_version: current,
                to_version: walk.stopped_at,
                sqlite_path: snap.sqlite_path.clone(),
                include_plugin_databases: snap.include_plugin_databases,
                sql_dump: snap.sql_dump.clone(),
            };
            snapshot_library(db, &req).await?;
        }
    }
    Ok(walk)
}

/// Applies a prepared walk using in-process atomic DDL.
async fn apply_walk_direct(
    db: &DatabaseConnection,
    kind: HostSchemaKind,
    walk: &SchemaWalk,
) -> Result<()> {
    let backend = db.get_database_backend();
    let timing = schema_migration_timing(backend);
    if kind == HostSchemaKind::PragmaMarker {
        exec_sql(db, backend, "PRAGMA foreign_keys = OFF").await?;
    }
    for step in &walk.ups {
        match kind {
            HostSchemaKind::PragmaMarker => {
                apply_one_sqlite_version(db, backend, step).await?;
            }
            HostSchemaKind::RowMarker | HostSchemaKind::AtomicBatchMarker => {
                apply_one_schema_migration(db, backend, timing, step).await?;
            }
        }
    }
    for step in &walk.downs {
        apply_one_down(db, backend, kind, timing, step).await?;
    }
    if kind == HostSchemaKind::PragmaMarker {
        exec_sql(db, backend, "PRAGMA foreign_keys = ON").await?;
    }
    Ok(())
}

/// Applies a prepared walk using a guest `run_batch` closure.
async fn apply_walk_batch<F, Fut>(
    db: &DatabaseConnection,
    kind: HostSchemaKind,
    walk: &SchemaWalk,
    run_batch: &mut F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let backend = db.get_database_backend();
    if kind == HostSchemaKind::PragmaMarker {
        exec_sql(db, backend, "PRAGMA foreign_keys = OFF").await?;
    }
    for step in &walk.ups {
        match kind {
            HostSchemaKind::PragmaMarker => {
                apply_one_sqlite_version_with_batch(db, step, run_batch).await?;
            }
            HostSchemaKind::RowMarker | HostSchemaKind::AtomicBatchMarker => {
                apply_one_schema_migration_with_batch(db, backend, step, run_batch).await?;
            }
        }
    }
    for step in &walk.downs {
        let stmts = down_statements(kind, step);
        run_batch(stmts).await?;
    }
    if kind == HostSchemaKind::PragmaMarker {
        exec_sql(db, backend, "PRAGMA foreign_keys = ON").await?;
    }
    Ok(())
}

/// Reads `PRAGMA user_version` or `max(schema_migrations.version)`.
///
/// # Errors
///
/// Returns when the version query fails.
pub async fn current_schema_version(db: &DatabaseConnection, kind: HostSchemaKind) -> Result<i64> {
    match kind {
        HostSchemaKind::PragmaMarker => sqlite_user_version(db).await,
        HostSchemaKind::RowMarker | HostSchemaKind::AtomicBatchMarker => {
            let applied = schema_versions_applied(db, db.get_database_backend()).await?;
            Ok(applied.into_iter().max().unwrap_or(0))
        }
    }
}

/// Canonical DDL plus version markers (`PRAGMA` and/or `schema_migrations` insert).
fn version_marker_statements(kind: HostSchemaKind, step: &HostMigrationStep) -> Vec<String> {
    let mut stmts = vec![step.canonical.to_string()];
    if kind == HostSchemaKind::PragmaMarker {
        stmts.push(format!("PRAGMA user_version = {}", step.version));
    }
    stmts.push(schema_migrations_insert(step));
    stmts
}

/// Reverse DDL plus deletion of this step's `schema_migrations` row (and pragma).
fn down_statements(kind: HostSchemaKind, step: &HostMigrationStep) -> Vec<String> {
    let mut stmts = Vec::new();
    if let Some(down) = step.down {
        stmts.push(down.to_string());
    }
    stmts.push(format!(
        "DELETE FROM schema_migrations WHERE version = {}",
        step.version
    ));
    if kind == HostSchemaKind::PragmaMarker {
        stmts.push(format!(
            "PRAGMA user_version = {}",
            step.version.saturating_sub(1)
        ));
    }
    stmts
}

/// `INSERT` for `schema_migrations` including checksum, app version, and timestamp.
fn schema_migrations_insert(step: &HostMigrationStep) -> String {
    let checksum = step.checksum();
    let app = env!("CARGO_PKG_VERSION").replace('\'', "''");
    let at = chrono::Utc::now().to_rfc3339().replace('\'', "''");
    format!(
        "INSERT INTO schema_migrations (version, checksum, app_version, applied_at) \
         VALUES ({}, '{checksum}', '{app}', '{at}')",
        step.version
    )
}

/// Refuses when a stored checksum does not match this binary's frozen SQL.
async fn verify_applied_checksums(
    db: &DatabaseConnection,
    backend: DbBackend,
    plan: &[HostMigrationStep],
    current: i64,
) -> Result<()> {
    if current <= 0 {
        return Ok(());
    }
    let rows = db
        .query_all_raw(Statement::from_string(
            backend,
            "SELECT version, checksum FROM schema_migrations",
        ))
        .await;
    let Ok(rows) = rows else {
        return Ok(());
    };
    for row in rows {
        let version = row
            .try_get::<i64>("", "version")
            .ok()
            .or_else(|| row.try_get_by_index::<i64>(0).ok())
            .unwrap_or(0);
        if version > current {
            continue;
        }
        let Some(step) = plan.iter().find(|s| s.version == version) else {
            continue;
        };
        let found = row
            .try_get::<String>("", "checksum")
            .ok()
            .or_else(|| row.try_get_by_index::<String>(1).ok())
            .unwrap_or_default();
        if found.is_empty() {
            continue;
        }
        let expected = step.checksum();
        if found != expected {
            return Err(LibraryError::Schema(format!(
                "schema version {version} checksum mismatch (database {found}, binary {expected}); \
                 the frozen migration was edited"
            )));
        }
    }
    Ok(())
}

/// Applies one reversible `down` as an atomic DDL batch.
async fn apply_one_down(
    db: &DatabaseConnection,
    backend: DbBackend,
    kind: HostSchemaKind,
    timing: &str,
    step: &HostMigrationStep,
) -> Result<()> {
    run_atomic_ddl(
        db,
        backend,
        timing,
        &format!("migrate-down-{}", step.version),
        down_statements(kind, step),
    )
    .await
}

/// Applies one SQLite `PRAGMA user_version` step via `run_batch`.
async fn apply_one_sqlite_version_with_batch<F, Fut>(
    db: &DatabaseConnection,
    step: &HostMigrationStep,
    run_batch: &mut F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let version = step.version;
    let mut delay_ms = 20u64;
    let mut last_applied_err = None;
    for attempt in 0..8 {
        if version <= sqlite_user_version(db).await? {
            return Ok(());
        }
        let stmts = version_marker_statements(HostSchemaKind::PragmaMarker, step);
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

/// Applies one `schema_migrations` step via `run_batch`.
async fn apply_one_schema_migration_with_batch<F, Fut>(
    db: &DatabaseConnection,
    backend: DbBackend,
    step: &HostMigrationStep,
    run_batch: &mut F,
) -> Result<()>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let version = step.version;
    let mut delay_ms = 20u64;
    let mut last_err = None;
    for attempt in 0..8 {
        if schema_versions_applied(db, backend)
            .await?
            .contains(&version)
        {
            return Ok(());
        }
        let stmts = version_marker_statements(HostSchemaKind::RowMarker, step);
        match run_batch(stmts).await {
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

/// Applies one file-SQLite version, recovering when a peer committed DDL first.
async fn apply_one_sqlite_version(
    db: &DatabaseConnection,
    backend: DbBackend,
    step: &HostMigrationStep,
) -> Result<()> {
    let version = step.version;
    let mut delay_ms = 20u64;
    let mut last_applied_err = None;
    for attempt in 0..8 {
        if version <= sqlite_user_version(db).await? {
            return Ok(());
        }
        match run_atomic_ddl(
            db,
            backend,
            "sqlite_txn",
            &format!("migrate-sqlite-{version}"),
            version_marker_statements(HostSchemaKind::PragmaMarker, step),
        )
        .await
        {
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

/// Applies one `schema_migrations` version, recovering from a concurrent peer.
async fn apply_one_schema_migration(
    db: &DatabaseConnection,
    backend: DbBackend,
    timing: &str,
    step: &HostMigrationStep,
) -> Result<()> {
    let version = step.version;
    let mut delay_ms = 20u64;
    let mut last_err = None;
    for attempt in 0..8 {
        if schema_versions_applied(db, backend)
            .await?
            .contains(&version)
        {
            return Ok(());
        }
        match run_atomic_ddl(
            db,
            backend,
            timing,
            &format!("migrate-{timing}-{version}"),
            version_marker_statements(HostSchemaKind::RowMarker, step),
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
            &bookclerk_db_exec::schema_sql_for_backend(backend, SCHEMA_MIGRATIONS_DDL),
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
        &bookclerk_db_exec::schema_sql_for_backend(backend, SCHEMA_MIGRATIONS_DDL),
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
    let stmts = bookclerk_db_exec::expand_host_schema_batch(backend, &stmts).unwrap_or(stmts);
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

/// Runs `stmts` as one generic atomic execute plan (version marker last).
fn is_already_applied_ddl(err: &LibraryError) -> bool {
    let s = format!("{err}\n{err:?}").to_lowercase();
    s.contains("duplicate column")
        || s.contains("already exists")
        || s.contains("duplicate key")
        || s.contains("23505")
        || s.contains("sqlite_constraint")
}

/// True when a concurrent migrator or ambiguous D1 commit should retry this version.
fn is_schema_lock_err(err: &LibraryError) -> bool {
    let s = format!("{err}\n{err:?}").to_ascii_lowercase();
    s.contains("sqlite_busy")
        || s.contains("sqlite_locked")
        || s.contains("database is locked")
        || s.contains("begin failed")
        || s.contains("40p01")
        || s.contains("40001")
        || s.contains("55p03")
        // D1 committed-but-lost / 5xx after commit: retry; version marker makes re-apply safe.
        || s.contains("d1 ambiguous")
        || s.contains("ambiguous response")
        || s.contains("commit reply lost")
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

/// Executes one SQL string.
async fn exec_sql(db: &DatabaseConnection, backend: DbBackend, sql: &str) -> Result<()> {
    db.execute_raw(Statement::from_string(backend, sql.to_string()))
        .await
        .map_err(LibraryError::Orm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookclerk_plugin_abi::DbCapabilities;

    #[test]
    fn host_migration_plan_starts_with_greenfield_baseline() {
        use crate::migrations::{
            greenfield_baseline_canonical, host_migration_plan, SCHEMA_VERSION,
        };
        let plan = host_migration_plan();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].version, SCHEMA_VERSION);
        assert_eq!(plan[0].canonical, greenfield_baseline_canonical());
        assert!(plan[0].canonical.contains("plugin_databases"));
        for (i, step) in plan.iter().enumerate() {
            assert_eq!(step.version, i as i64 + 1);
        }
    }

    #[test]
    fn from_db_capabilities_selects_kind_from_flags_not_identity() {
        assert_eq!(
            HostSchemaKind::from_db_capabilities(&DbCapabilities::advertised_sqlite()).unwrap(),
            HostSchemaKind::PragmaMarker
        );
        assert_eq!(
            HostSchemaKind::from_db_capabilities(&DbCapabilities::advertised_postgres()).unwrap(),
            HostSchemaKind::RowMarker
        );
        assert_eq!(
            HostSchemaKind::from_db_capabilities(&DbCapabilities::advertised_d1()).unwrap(),
            HostSchemaKind::AtomicBatchMarker
        );

        let mut mixed = DbCapabilities::advertised_sqlite();
        mixed.schema_migrations = true;
        assert!(HostSchemaKind::from_db_capabilities(&mixed).is_err());
        let mut none = DbCapabilities::advertised_sqlite();
        none.pragma_user_version = false;
        assert!(HostSchemaKind::from_db_capabilities(&none).is_err());
        assert!(HostSchemaKind::PragmaMarker
            .advertised_db_capabilities_match(&none)
            .is_err());
    }

    #[tokio::test]
    async fn schema_migrations_on_sqlite_uses_canonical_sql_not_postgres_pack() {
        let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
            .await
            .expect("unmigrated sqlite");
        apply_host_schema(&db, HostSchemaKind::RowMarker)
            .await
            .expect("canonical sqlite pack on sqlite backend");
        let cols = db
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(books)",
            ))
            .await
            .expect("table_info");
        assert!(
            !cols.is_empty(),
            "canonical SQLITE_SCHEMA must create books"
        );
    }

    #[tokio::test]
    async fn atomic_batch_marker_applies_canonical_plan() {
        let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
            .await
            .expect("unmigrated sqlite");
        let db_batch = db.clone();
        apply_host_schema_with_batch(&db, HostSchemaKind::AtomicBatchMarker, move |stmts| {
            let db_batch = db_batch.clone();
            async move {
                run_atomic_ddl(
                    &db_batch,
                    db_batch.get_database_backend(),
                    "sqlite_txn",
                    "atomic-batch",
                    stmts,
                )
                .await
            }
        })
        .await
        .expect("atomic batch schema");
        let plan = host_migration_plan();
        let applied = schema_versions_applied(&db, DbBackend::Sqlite)
            .await
            .unwrap();
        let last = plan.last().map(|s| s.version).unwrap_or(0);
        assert!(
            applied.contains(&last),
            "last plan version {last}, applied={applied:?}"
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
        let ddl = bookclerk_db_exec::split_schema_statements(host_migration_plan()[0].canonical)
            .len() as u32;
        crate::inject_atomic_interrupt_after(
            crate::AtomicInterruptPhase::BetweenStatements,
            crate::AtomicInterruptKind::Cancel,
            ddl,
        );
        let err = apply_host_schema(&db, HostSchemaKind::PragmaMarker)
            .await
            .expect_err("interrupt before version marker");
        assert!(err.to_string().to_lowercase().contains("cancel"), "{err}");
        assert_eq!(sqlite_user_version(&db).await.unwrap(), 0);
        apply_host_schema(&db, HostSchemaKind::PragmaMarker)
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
            apply_host_schema(&db1, HostSchemaKind::PragmaMarker),
            apply_host_schema(&db2, HostSchemaKind::PragmaMarker),
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
        let canonical = host_migration_plan()[0].canonical;
        let ddl = bookclerk_db_exec::expand_host_schema_batch(
            DbBackend::Postgres,
            &[
                canonical.to_string(),
                "INSERT INTO schema_migrations (version) VALUES (1)".to_string(),
            ],
        )
        .expect("postgres schema batch")
        .len()
        .saturating_sub(1) as u32;
        crate::inject_atomic_interrupt_after(
            crate::AtomicInterruptPhase::BetweenStatements,
            crate::AtomicInterruptKind::Cancel,
            ddl,
        );
        let err = apply_host_schema(&db, HostSchemaKind::RowMarker)
            .await
            .expect_err("interrupt");
        assert!(err.to_string().to_lowercase().contains("cancel"), "{err}");
        apply_host_schema(&db, HostSchemaKind::RowMarker)
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
            apply_host_schema(&db1, HostSchemaKind::RowMarker),
            apply_host_schema(&db2, HostSchemaKind::RowMarker),
        );
        a.expect("first apply");
        b.expect("second apply");
    }

    #[tokio::test]
    async fn pragma_marker_fails_closed_when_database_is_ahead() {
        let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
            .await
            .expect("unmigrated sqlite");
        apply_host_schema(&db, HostSchemaKind::PragmaMarker)
            .await
            .expect("apply v1");
        exec_sql(&db, DbBackend::Sqlite, "PRAGMA user_version = 99")
            .await
            .expect("bump");
        let err = apply_host_schema(&db, HostSchemaKind::PragmaMarker)
            .await
            .expect_err("schema ahead");
        assert!(err.to_string().contains("newer than this binary"), "{err}");
    }

    #[tokio::test]
    async fn checksum_mismatch_fails_closed() {
        let db = bookclerk_plugin_database_sqlite::open_memory_unmigrated()
            .await
            .expect("unmigrated sqlite");
        apply_host_schema(&db, HostSchemaKind::PragmaMarker)
            .await
            .expect("apply v1");
        exec_sql(
            &db,
            DbBackend::Sqlite,
            "UPDATE schema_migrations SET checksum = 'deadbeef' WHERE version = 1",
        )
        .await
        .expect("tamper");
        let err = apply_host_schema(&db, HostSchemaKind::PragmaMarker)
            .await
            .expect_err("checksum");
        assert!(err.to_string().contains("checksum mismatch"), "{err}");
    }
}
