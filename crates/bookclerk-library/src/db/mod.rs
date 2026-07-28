//! Database plugin connections (SeaORM).
//!
//! Built-in backends:
//! - `sqlite` — local file through rusqlite (proxy; default)
//! - `d1` — Cloudflare D1 over the HTTP API (proxy)
//! - `postgres` — PostgreSQL via sqlx-postgres (native sqlx connection)
//!
//! SeaORM is the query layer for [`crate::LibraryStore`]: every backend is a
//! [`DatabaseConnection`] proxy or native connection, and the store issues SQL
//! through [`ConnectionTrait`]. Local SQLite files migrate with
//! `rusqlite_migration` (`PRAGMA user_version`); D1 and Postgres apply the
//! same migration texts via [`apply_pending_migrations`].

mod d1;
mod postgres;
mod runtime;
mod sqlite;

use std::path::Path;
use std::sync::Arc;

use bookclerk_config::{Config, DatabasePluginKind};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement, Value};

use crate::error::{LibraryError, Result};
use crate::migrations;

pub use d1::{resolve_d1_api_token, D1Proxy};
pub use postgres::{connect_postgres, resolve_postgres_url};
pub use runtime::block_on_db;
pub use sqlite::SqliteProxy;

/// Open the configured database plugin.
pub async fn connect_from_config(config: &Config) -> Result<DatabaseConnection> {
    match config.database.active_plugin()? {
        DatabasePluginKind::Sqlite => {
            let path = config.database.sqlite_path(&config.paths().files_dir);
            connect_sqlite(&path).await
        }
        DatabasePluginKind::D1 => connect_d1(config).await,
        DatabasePluginKind::Postgres => connect_postgres(config).await,
    }
}

/// Open (or create) a local SQLite database, migrate, return a SeaORM proxy.
pub async fn connect_sqlite(path: &Path) -> Result<DatabaseConnection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = rusqlite::Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrations::migrations().to_latest(&mut conn)?;
    let db = Database::connect_proxy(
        DbBackend::Sqlite,
        Arc::new(Box::new(SqliteProxy::new(conn))),
    )
    .await
    .map_err(LibraryError::Orm)?;
    tracing::debug!(path = %path.display(), plugin = "sqlite", "opened library database (sea-orm proxy)");
    Ok(db)
}

/// Open an in-memory SQLite database, migrate, return a SeaORM proxy.
///
/// Same wrapping as [`connect_sqlite`] but without a backing file (tests).
pub async fn connect_sqlite_memory() -> Result<DatabaseConnection> {
    let mut conn = rusqlite::Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrations::migrations().to_latest(&mut conn)?;
    let db = Database::connect_proxy(
        DbBackend::Sqlite,
        Arc::new(Box::new(SqliteProxy::new(conn))),
    )
    .await
    .map_err(LibraryError::Orm)?;
    tracing::debug!(
        plugin = "sqlite",
        "opened in-memory library database (sea-orm proxy)"
    );
    Ok(db)
}

/// Open Cloudflare D1 through the HTTP API proxy.
pub async fn connect_d1(config: &Config) -> Result<DatabaseConnection> {
    let token = resolve_d1_api_token(config)?;
    let proxy = D1Proxy::new(
        config.database.d1.api_base.clone(),
        config.database.d1.account_id.clone(),
        config.database.d1.database_id.clone(),
        token,
    );
    let db = Database::connect_proxy(DbBackend::Sqlite, Arc::new(Box::new(proxy)))
        .await
        .map_err(LibraryError::Orm)?;
    db.ping().await.map_err(LibraryError::Orm)?;
    apply_pending_migrations(&db).await?;
    tracing::debug!(plugin = "d1", "opened library database (sea-orm proxy)");
    Ok(db)
}

/// Apply any un-applied schema migrations through SeaORM.
///
/// For back-ends that cannot use `rusqlite_migration` (D1, Postgres) we track
/// applied versions in a `schema_migrations` table. D1 replays the SQLite
/// migration texts from [`migrations::migration_sql`]. Fresh Postgres databases
/// apply [`migrations::postgres_bootstrap_schema`] once and mark every current
/// version as applied (historical SQLite rebuilds are not portable).
pub async fn apply_pending_migrations(db: &DatabaseConnection) -> Result<()> {
    let backend = db.get_database_backend();
    db.execute_raw(Statement::from_string(
        backend,
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY)",
    ))
    .await
    .map_err(LibraryError::Orm)?;

    let applied: std::collections::HashSet<i64> = db
        .query_all_raw(Statement::from_string(
            backend,
            "SELECT version FROM schema_migrations",
        ))
        .await
        .map_err(LibraryError::Orm)?
        .iter()
        .filter_map(|row| row.try_get::<i64>("", "version").ok())
        .collect();

    if backend == DbBackend::Postgres && applied.is_empty() {
        for stmt in split_sql_statements(migrations::postgres_bootstrap_schema()) {
            db.execute_raw(Statement::from_string(backend, stmt))
                .await
                .map_err(LibraryError::Orm)?;
        }
        for version in 1..=migrations::migration_sql().len() as i64 {
            db.execute_raw(Statement::from_sql_and_values(
                backend,
                "INSERT INTO schema_migrations (version) VALUES ($1)",
                [Value::from(version)],
            ))
            .await
            .map_err(LibraryError::Orm)?;
        }
        return Ok(());
    }

    for (idx, sql) in migrations::migration_sql().iter().enumerate() {
        let version = idx as i64 + 1;
        if applied.contains(&version) {
            continue;
        }
        if backend == DbBackend::Postgres {
            return Err(LibraryError::Other(anyhow::anyhow!(
                "Postgres schema is behind (missing migration {version}); \
                 greenfield DBs bootstrap automatically — for upgrades, \
                 apply the new DDL manually or recreate the database \
                 (see docs/database.md)"
            )));
        }
        for stmt in split_sql_statements(sql) {
            db.execute_raw(Statement::from_string(backend, stmt))
                .await
                .map_err(LibraryError::Orm)?;
        }
        db.execute_raw(Statement::from_sql_and_values(
            backend,
            "INSERT INTO schema_migrations (version) VALUES (?)",
            [Value::from(version)],
        ))
        .await
        .map_err(LibraryError::Orm)?;
    }
    Ok(())
}

/// Split a migration text into individual statements on `;`.
///
/// The migration SQL never embeds `;` inside string literals, so a simple split
/// is sufficient (and avoids relying on multi-statement support in the D1 HTTP
/// API).
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

    #[tokio::test]
    async fn connect_sqlite_file_and_ping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let db = connect_sqlite(&path).await.unwrap();
        db.ping().await.unwrap();
        assert!(path.is_file());
    }
}
