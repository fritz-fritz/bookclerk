//! Database plugin connections (SeaORM).
//!
//! Built-in backends (both via SeaORM `ProxyDatabaseTrait`):
//! - `sqlite` — local file through rusqlite (default)
//! - `d1` — Cloudflare D1 over the HTTP API
//!
//! SeaORM is the long-term query layer (entities / ActiveModel). Local
//! `LibraryStore` methods remain rusqlite-backed until that migration lands;
//! D1 is reachable for ping / probes through [`connect_from_config`].

mod d1;
mod runtime;
mod sqlite;

use std::path::Path;
use std::sync::Arc;

use bookclerk_config::{Config, DatabasePluginKind};
use sea_orm::{Database, DatabaseConnection, DbBackend};

use crate::error::{LibraryError, Result};
use crate::migrations;

pub use d1::{resolve_d1_api_token, D1Proxy};
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
    tracing::debug!(plugin = "d1", "opened library database (sea-orm proxy)");
    Ok(db)
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
