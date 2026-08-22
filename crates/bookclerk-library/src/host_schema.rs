//! Host-owned schema application after a database guest connects.
//!
//! Guests open a connection and ping. The host reads the current version and
//! sends remaining DDL as generic `execute` statements (D1 V27 as one atomic
//! batch).

use std::collections::HashSet;
use std::future::Future;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::error::{LibraryError, Result};
use crate::migrations::{
    migration_sql, migration_sql_d1, migration_sql_postgres, migration_v27_d1_batch,
    migration_v27_schema_version,
};

/// Which versioning table / dialect the host should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSchemaKind {
    /// Local SQLite files (`PRAGMA user_version`).
    SqliteFile,
    /// PostgreSQL (`schema_migrations` + Postgres DDL).
    Postgres,
    /// Cloudflare D1 (`schema_migrations` + sqlite DDL; V27 is one batch).
    D1,
}

impl HostSchemaKind {
    /// Maps a database plugin id onto a schema target.
    #[must_use]
    pub fn from_plugin_id(plugin_id: &str) -> Self {
        match plugin_id.trim().to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" => Self::Postgres,
            "d1" => Self::D1,
            _ => Self::SqliteFile,
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
            apply_schema_migrations(db, DbBackend::Postgres, migration_sql_postgres()).await
        }
        HostSchemaKind::D1 => {
            apply_schema_migrations(db, DbBackend::Sqlite, migration_sql_d1()).await?;
            let v27 = migration_v27_schema_version();
            if !schema_version_applied(db, DbBackend::Sqlite, v27).await? {
                return Err(LibraryError::Other(anyhow::anyhow!(
                    "D1 V27 must be applied as one atomic batch (apply_host_schema_with_batch)"
                )));
            }
            apply_schema_migrations(db, DbBackend::Sqlite, migration_sql()).await
        }
    }
}

/// Applies pending DDL, running D1 V27 through `run_batch` as one SQL transaction.
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
        return apply_host_schema(db, kind).await;
    }
    apply_schema_migrations(db, DbBackend::Sqlite, migration_sql_d1()).await?;
    let v27 = migration_v27_schema_version();
    if !schema_version_applied(db, DbBackend::Sqlite, v27).await? {
        run_batch(migration_v27_d1_batch()).await?;
    }
    apply_schema_migrations(db, DbBackend::Sqlite, migration_sql()).await
}

/// Applies remaining `PRAGMA user_version` steps (file SQLite).
async fn apply_sqlite_user_version(db: &DatabaseConnection) -> Result<()> {
    let backend = DbBackend::Sqlite;
    exec_sql(db, backend, "PRAGMA foreign_keys = OFF").await?;
    let current = sqlite_user_version(db).await?;
    let steps = migration_sql();
    for (idx, schema) in steps.iter().enumerate() {
        let version = (idx + 1) as i64;
        if version <= current {
            continue;
        }
        for stmt in split_sql_statements(schema) {
            exec_sql(db, backend, &stmt).await?;
        }
        exec_sql(db, backend, &format!("PRAGMA user_version = {version}")).await?;
    }
    exec_sql(db, backend, "PRAGMA foreign_keys = ON").await?;
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
    steps: &[&str],
) -> Result<()> {
    exec_sql(
        db,
        backend,
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY)",
    )
    .await?;
    let applied = schema_versions_applied(db, backend).await?;
    let insert = if backend == DbBackend::Postgres {
        "INSERT INTO schema_migrations (version) VALUES ($1)"
    } else {
        "INSERT INTO schema_migrations (version) VALUES (?)"
    };
    for (idx, schema) in steps.iter().enumerate() {
        let version = (idx + 1) as i64;
        if applied.contains(&version) {
            continue;
        }
        for stmt in split_sql_statements(schema) {
            exec_sql(db, backend, &stmt).await?;
        }
        db.execute_raw(Statement::from_sql_and_values(
            backend,
            insert,
            [Value::from(version)],
        ))
        .await
        .map_err(LibraryError::Orm)?;
    }
    Ok(())
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
        .filter_map(|row| row.try_get::<i64>("", "version").ok())
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
