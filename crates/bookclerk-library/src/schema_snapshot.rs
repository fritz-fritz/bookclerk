//! In-place library snapshots taken before host schema ups and downs.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::Connection;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::error::{LibraryError, Result};
use crate::migrations::{latest_schema_postgres, latest_schema_sqlite, SCHEMA_VERSION};

/// Subdirectory of the files dir that holds automatic snapshots.
pub const SNAPSHOTS_DIR: &str = "snapshots";

/// Automatic snapshots retained after each prune.
pub const SNAPSHOT_RETENTION: usize = 5;

/// Host tables dumped for non-file backends (order does not need FK safety;
/// restore reapplies frozen DDL then inserts).
const HOST_DUMP_TABLES: &[&str] = &[
    "accounts",
    "books",
    "ignored_titles",
    "saved_filters",
    "users",
    "portal_identities",
    "claim_tickets",
    "portal_sessions",
    "operator_sessions",
    "security_audit_events",
    "account_links",
    "works",
    "work_editions",
    "listening_progress",
    "title_requests",
    "title_request_sources",
    "embeddings",
    "user_preferences",
    "encrypted_secrets",
    "user_invites",
    "oidc_clients",
    "oidc_auth_codes",
    "oidc_refresh_tokens",
    "oidc_rp_states",
    "webauthn_credentials",
    "webauthn_challenges",
    "db_atomic_receipts",
    "jobs",
    "job_temp_paths",
    "job_queue_control",
    "domain_events",
    "event_deliveries",
    "event_subscriber_nodes",
    "event_outbox_stats",
    "db_serialization_slots",
    "plugin_databases",
    "schema_migrations",
];

/// How a snapshot was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    /// SQLite `VACUUM INTO` of `library.db`.
    SqliteVacuum,
    /// SQL dump through the live guest connection (Postgres / D1).
    SqlDump,
}

/// On-disk snapshot metadata (`manifest.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    /// Schema version recorded when the snapshot was taken.
    pub schema_version: i64,
    /// Bookclerk version that wrote the snapshot.
    pub app_version: String,
    /// RFC 3339 UTC timestamp.
    pub created_at: String,
    /// Capture mechanic.
    pub kind: SnapshotKind,
    /// Target schema version the following migrate step was heading toward.
    pub migrate_to: i64,
    /// When true, plugin binding units were copied or dumped too.
    pub include_plugin_databases: bool,
}

/// Inputs for an automatic or CLI snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotRequest {
    /// `$BOOKCLERK_FILES_DIR`.
    pub files_dir: PathBuf,
    /// Current database schema version.
    pub from_version: i64,
    /// Version the upcoming migrate step will target.
    pub to_version: i64,
    /// File SQLite path for `VACUUM INTO`; `None` uses a SQL dump.
    pub sqlite_path: Option<PathBuf>,
    /// Copy `plugin-databases/` (SQLite files) when present.
    pub include_plugin_databases: bool,
    /// Precomputed SQL dump (D1 REST export). When set, written as `library.sql`.
    pub sql_dump: Option<Vec<u8>>,
}

/// Result of writing a snapshot directory.
#[derive(Debug, Clone)]
pub struct SnapshotOutcome {
    /// Directory under `files_dir/snapshots/`.
    pub dir: PathBuf,
    /// Parsed manifest.
    pub manifest: SnapshotManifest,
}

/// Consistent SQLite copy via `VACUUM INTO`.
///
/// # Errors
///
/// Returns [`LibraryError::Db`] when SQLite rejects the backup.
pub fn vacuum_sqlite_into(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        fs::remove_file(dest)?;
    }
    let conn = Connection::open(src)?;
    let dest_s = dest.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{dest_s}'"))?;
    Ok(())
}

/// Writes an in-place snapshot under `files_dir/snapshots/` and prunes old ones.
///
/// Empty databases (`from_version == 0`) skip the snapshot.
///
/// # Errors
///
/// Returns when the files dir cannot be written or dump/vacuum fails.
pub async fn snapshot_library(
    db: &DatabaseConnection,
    req: &SnapshotRequest,
) -> Result<Option<SnapshotOutcome>> {
    if req.from_version <= 0 {
        return Ok(None);
    }
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let name = format!(
        "{stamp}-pre-schema-{}-to-{}",
        req.from_version, req.to_version
    );
    let dir = req.files_dir.join(SNAPSHOTS_DIR).join(name);
    fs::create_dir_all(&dir)?;

    let kind = if let Some(bytes) = req.sql_dump.as_ref() {
        fs::write(dir.join("library.sql"), bytes)?;
        SnapshotKind::SqlDump
    } else if let Some(sqlite_path) = req.sqlite_path.as_ref() {
        if sqlite_path.is_file() {
            vacuum_sqlite_into(sqlite_path, &dir.join("library.db"))?;
            SnapshotKind::SqliteVacuum
        } else {
            dump_sql(db, &dir.join("library.sql")).await?;
            SnapshotKind::SqlDump
        }
    } else {
        dump_sql(db, &dir.join("library.sql")).await?;
        SnapshotKind::SqlDump
    };

    if req.include_plugin_databases {
        let src = req.files_dir.join("plugin-databases");
        if src.is_dir() {
            copy_dir_recursive(&src, &dir.join("plugin-databases"))?;
        }
        dump_postgres_plugin_schemas(db, &dir.join("plugin-databases")).await?;
    }

    let manifest = SnapshotManifest {
        schema_version: req.from_version,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: Utc::now().to_rfc3339(),
        kind,
        migrate_to: req.to_version,
        include_plugin_databases: req.include_plugin_databases,
    };
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)
            .map_err(|err| LibraryError::Other(anyhow::anyhow!("snapshot manifest json: {err}")))?,
    )?;
    prune_snapshots(&req.files_dir.join(SNAPSHOTS_DIR))?;
    Ok(Some(SnapshotOutcome { dir, manifest }))
}

/// Restores `library.db` / `library.sql` from a snapshot directory.
///
/// # Errors
///
/// Returns when the snapshot is missing or the copy/apply fails.
pub async fn restore_snapshot(
    db: &DatabaseConnection,
    snapshot_dir: &Path,
    sqlite_path: Option<&Path>,
) -> Result<SnapshotManifest> {
    let manifest: SnapshotManifest =
        serde_json::from_slice(&fs::read(snapshot_dir.join("manifest.json"))?)
            .map_err(|err| LibraryError::Other(anyhow::anyhow!("snapshot manifest: {err}")))?;
    match manifest.kind {
        SnapshotKind::SqliteVacuum => {
            let src = snapshot_dir.join("library.db");
            let dest = sqlite_path.ok_or_else(|| {
                LibraryError::Schema("sqlite snapshot restore requires a library.db path".into())
            })?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src, dest)?;
        }
        SnapshotKind::SqlDump => {
            let sql = fs::read_to_string(snapshot_dir.join("library.sql"))?;
            let backend = db.get_database_backend();
            for stmt in bookclerk_db_exec::split_schema_statements(&sql) {
                db.execute_raw(Statement::from_string(backend, stmt))
                    .await
                    .map_err(LibraryError::Orm)?;
            }
        }
    }
    let plugin_src = snapshot_dir.join("plugin-databases");
    if plugin_src.is_dir() {
        if let Some(dest_root) = sqlite_path.and_then(|p| p.parent()) {
            let dest = dest_root.join("plugin-databases");
            if dest.exists() {
                fs::remove_dir_all(&dest)?;
            }
            copy_dir_recursive(&plugin_src, &dest)?;
        }
    }
    Ok(manifest)
}

/// Packages a snapshot directory as `.tar.gz`. Operator archives are never pruned.
///
/// # Errors
///
/// Returns when the destination cannot be written.
pub fn archive_snapshot_dir(dir: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(dest)?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(enc);
    builder.append_dir_all(".", dir)?;
    let enc = builder
        .into_inner()
        .map_err(|err| LibraryError::Other(anyhow::anyhow!("snapshot tar: {err}")))?;
    enc.finish()
        .map_err(|err| LibraryError::Other(anyhow::anyhow!("snapshot gzip: {err}")))?;
    Ok(())
}

/// Extracts a `.tar.gz` snapshot archive into `dest`.
///
/// # Errors
///
/// Returns when the archive is missing, malformed, or a path escapes `dest`.
pub fn extract_snapshot_archive(archive: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    let file = fs::File::open(archive)?;
    let dec = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(dec);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(LibraryError::Schema(format!(
                "snapshot archive path escapes destination: {}",
                path.display()
            )));
        }
        entry.unpack(dest.join(&path))?;
    }
    Ok(())
}

/// SQL dump of host tables through the live connection.
async fn dump_sql(db: &DatabaseConnection, dest: &Path) -> Result<()> {
    let backend = db.get_database_backend();
    let mut out = String::from("-- bookclerk host schema snapshot\n");
    out.push_str(&format!("-- schema_version {SCHEMA_VERSION}\n"));
    match backend {
        DbBackend::Postgres => {
            out.push_str(&latest_schema_postgres());
            out.push_str(";\n");
        }
        _ => {
            out.push_str(latest_schema_sqlite());
            out.push('\n');
        }
    }
    for table in HOST_DUMP_TABLES {
        let cols = match table_columns(db, backend, table).await {
            Ok(cols) if !cols.is_empty() => cols,
            _ => continue,
        };
        let select = format!("SELECT * FROM {table}");
        let rows = match db
            .query_all_raw(Statement::from_string(backend, select))
            .await
        {
            Ok(rows) => rows,
            Err(_) => continue,
        };
        for row in rows {
            out.push_str(&insert_from_named_row(backend, table, &cols, &row));
            out.push('\n');
        }
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(dest, out)?;
    Ok(())
}

/// Column names for `table` on `backend` (`PRAGMA table_info` or information_schema).
async fn table_columns(
    db: &DatabaseConnection,
    backend: DbBackend,
    table: &str,
) -> Result<Vec<String>> {
    let sql = match backend {
        DbBackend::Postgres => format!(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = '{table}' \
             ORDER BY ordinal_position"
        ),
        _ => format!("SELECT name FROM pragma_table_info('{table}')"),
    };
    let rows = db
        .query_all_raw(Statement::from_string(backend, sql))
        .await
        .map_err(LibraryError::Orm)?;
    let mut cols = Vec::new();
    for row in rows {
        let name = match backend {
            DbBackend::Postgres => row
                .try_get::<String>("", "column_name")
                .ok()
                .or_else(|| row.try_get_by_index::<String>(0).ok()),
            _ => row
                .try_get::<String>("", "name")
                .ok()
                .or_else(|| row.try_get_by_index::<String>(1).ok()),
        };
        if let Some(name) = name {
            cols.push(name);
        }
    }
    Ok(cols)
}

/// Formats one `INSERT INTO {table} VALUES (…)` from a named SeaORM row.
fn insert_from_named_row(
    backend: DbBackend,
    table: &str,
    cols: &[String],
    row: &QueryResult,
) -> String {
    let mut values = Vec::new();
    for name in cols {
        values.push(sql_literal_named(backend, row, name));
    }
    format!("INSERT INTO {table} VALUES ({});", values.join(", "))
}

/// SQL literal for one named column, including Postgres `bytea` vs SQLite blob.
fn sql_literal_named(backend: DbBackend, row: &QueryResult, name: &str) -> String {
    if let Ok(v) = row.try_get::<Option<String>>("", name) {
        return match v {
            Some(s) => format!("'{}'", s.replace('\'', "''")),
            None => "NULL".into(),
        };
    }
    if let Ok(v) = row.try_get::<Option<i64>>("", name) {
        return match v {
            Some(n) => n.to_string(),
            None => "NULL".into(),
        };
    }
    if let Ok(v) = row.try_get::<Option<f64>>("", name) {
        return match v {
            Some(n) => n.to_string(),
            None => "NULL".into(),
        };
    }
    if let Ok(v) = row.try_get::<Option<Vec<u8>>>("", name) {
        return match v {
            Some(bytes) if backend == DbBackend::Postgres => {
                format!("'\\x{}'::bytea", hex::encode(bytes))
            }
            Some(bytes) => format!("X'{}'", hex::encode(bytes)),
            None => "NULL".into(),
        };
    }
    "NULL".into()
}

/// Dumps Postgres `pb_*` plugin-binding schemas into `dest`.
async fn dump_postgres_plugin_schemas(db: &DatabaseConnection, dest: &Path) -> Result<()> {
    if db.get_database_backend() != DbBackend::Postgres {
        return Ok(());
    }
    let rows = match db
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT nspname FROM pg_namespace WHERE nspname LIKE 'pb_%' ORDER BY nspname",
        ))
        .await
    {
        Ok(rows) => rows,
        Err(_) => return Ok(()),
    };
    for row in rows {
        let Some(schema) = row
            .try_get::<String>("", "nspname")
            .ok()
            .or_else(|| row.try_get_by_index::<String>(0).ok())
        else {
            continue;
        };
        let tables = match db
            .query_all_raw(Statement::from_string(
                DbBackend::Postgres,
                format!(
                    "SELECT table_name FROM information_schema.tables \
                     WHERE table_schema = '{schema}' AND table_type = 'BASE TABLE' \
                     ORDER BY table_name"
                ),
            ))
            .await
        {
            Ok(t) => t,
            Err(_) => continue,
        };
        let mut out = format!("-- plugin schema {schema}\n");
        for table_row in tables {
            let Some(table) = table_row
                .try_get::<String>("", "table_name")
                .ok()
                .or_else(|| table_row.try_get_by_index::<String>(0).ok())
            else {
                continue;
            };
            let qualified = format!("{schema}.{table}");
            let col_rows = match db
                .query_all_raw(Statement::from_string(
                    DbBackend::Postgres,
                    format!(
                        "SELECT column_name FROM information_schema.columns \
                         WHERE table_schema = '{schema}' AND table_name = '{table}' \
                         ORDER BY ordinal_position"
                    ),
                ))
                .await
            {
                Ok(rows) => rows,
                Err(_) => continue,
            };
            let cols: Vec<String> = col_rows
                .iter()
                .filter_map(|r| {
                    r.try_get::<String>("", "column_name")
                        .ok()
                        .or_else(|| r.try_get_by_index::<String>(0).ok())
                })
                .collect();
            if cols.is_empty() {
                continue;
            }
            let select = format!("SELECT * FROM {qualified}");
            let data = match db
                .query_all_raw(Statement::from_string(DbBackend::Postgres, select))
                .await
            {
                Ok(rows) => rows,
                Err(_) => continue,
            };
            for data_row in data {
                out.push_str(&insert_from_named_row(
                    DbBackend::Postgres,
                    &qualified,
                    &cols,
                    &data_row,
                ));
                out.push('\n');
            }
        }
        fs::create_dir_all(dest)?;
        fs::write(dest.join(format!("{schema}.sql")), out)?;
    }
    Ok(())
}

/// Recursively copies `src` into `dest`.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if from.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Deletes oldest automatic snapshot directories beyond [`SNAPSHOT_RETENTION`].
fn prune_snapshots(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let extra = dirs.len().saturating_sub(SNAPSHOT_RETENTION);
    for old in dirs.into_iter().take(extra) {
        let _ = fs::remove_dir_all(old);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vacuum_sqlite_into_round_trips_user_version() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("library.db");
        let dest = dir.path().join("snap.db");
        let conn = Connection::open(&src).unwrap();
        conn.execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER);
             INSERT INTO t (n) VALUES (7);
             PRAGMA user_version = 1;",
        )
        .unwrap();
        drop(conn);
        vacuum_sqlite_into(&src, &dest).unwrap();
        let snap = Connection::open(&dest).unwrap();
        let n: i64 = snap.query_row("SELECT n FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 7);
        let v: i64 = snap
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);

        let packed = dir.path().join("snap.tar.gz");
        let unpacked = dir.path().join("unpacked");
        std::fs::create_dir_all(dir.path().join("snapdir")).unwrap();
        std::fs::copy(&dest, dir.path().join("snapdir/library.db")).unwrap();
        archive_snapshot_dir(&dir.path().join("snapdir"), &packed).unwrap();
        extract_snapshot_archive(&packed, &unpacked).unwrap();
        assert!(unpacked.join("library.db").is_file());
    }
}
