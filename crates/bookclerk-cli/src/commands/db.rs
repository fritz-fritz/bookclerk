//! `bookclerk db` — schema version, snapshots, migrate, and last-reversible downgrade.

use std::path::{Path, PathBuf};

use bookclerk_config::{Config, DatabasePluginKind};
use bookclerk_library::migrations::{host_migration_plan, SCHEMA_V1_INTRODUCED_IN};
use bookclerk_library::{
    archive_snapshot_dir, current_schema_version, extract_snapshot_archive, migrate_host_schema_to,
    restore_snapshot, snapshot_library, HostSchemaKind, SchemaApplyOptions, SchemaSnapshotOpts,
    SnapshotRequest, SCHEMA_VERSION,
};
use clap::Subcommand;
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::format_out::{emit, OutputFormat};

#[derive(Debug, Subcommand)]
/// Host schema versioning, snapshots, and explicit ups/downs.
pub enum DbCommand {
    /// Show this binary's schema version and the database's current version.
    Version,
    /// Write an in-place snapshot under `files_dir/snapshots/` (or `--path`).
    Snapshot {
        /// Optional `.tar.gz` / directory destination; default is in-place.
        #[arg(long)]
        path: Option<PathBuf>,
        /// Also copy `plugin-databases/` (plugin-owned DDL is not migrated).
        #[arg(long)]
        include_plugin_databases: bool,
    },
    /// Restore `library.db` (and plugin DBs when present) from a snapshot dir or `.tar.gz`.
    Restore {
        /// Snapshot directory or `.tar.gz` archive that contains `manifest.json`.
        #[arg(long)]
        from: PathBuf,
    },
    /// Apply ups or reversible downs to reach `--to`.
    Migrate {
        /// Target schema version (defaults to this binary's [`SCHEMA_VERSION`]).
        #[arg(long)]
        to: Option<i64>,
        /// Snapshot plugin binding databases as well.
        #[arg(long)]
        include_plugin_databases: bool,
    },
    /// Roll back toward this binary's schema version, stopping at the last reversible step.
    Downgrade {
        /// Snapshot plugin binding databases as well.
        #[arg(long)]
        include_plugin_databases: bool,
    },
}

/// Dispatches a `bookclerk db` verb.
pub async fn run(command: DbCommand, config: &Config, format: OutputFormat) -> anyhow::Result<()> {
    match command {
        DbCommand::Version => run_version(config, format).await,
        DbCommand::Snapshot {
            path,
            include_plugin_databases,
        } => run_snapshot(config, format, path, include_plugin_databases).await,
        DbCommand::Restore { from } => run_restore(config, format, from).await,
        DbCommand::Migrate {
            to,
            include_plugin_databases,
        } => {
            run_migrate(
                config,
                format,
                to.unwrap_or(SCHEMA_VERSION),
                include_plugin_databases,
            )
            .await
        }
        DbCommand::Downgrade {
            include_plugin_databases,
        } => run_migrate(config, format, SCHEMA_VERSION, include_plugin_databases).await,
    }
}

/// Opens the library guest without applying host schema (CLI migrate / version).
async fn open_unmigrated(
    config: &Config,
) -> anyhow::Result<(DatabaseConnection, HostSchemaKind, Option<PathBuf>)> {
    let registry = bookclerk_plugin_host::load_external_database(config).await?;
    let ext = registry.active().ok_or_else(|| {
        anyhow::anyhow!("no active database plugin — stage and enable [database].plugin")
    })?;
    let (db, caps) = ext.connect_without_migrate(config).await?;
    let kind = HostSchemaKind::from_db_capabilities(&caps)?;
    let sqlite_path = sqlite_path_opt(config);
    Ok((db, kind, sqlite_path))
}

/// Library SQLite path when `[database].plugin = sqlite`.
fn sqlite_path_opt(config: &Config) -> Option<PathBuf> {
    match DatabasePluginKind::parse(&config.database.plugin) {
        Some(DatabasePluginKind::Sqlite) => {
            Some(config.database.sqlite_path(&config.paths().files_dir))
        }
        _ => None,
    }
}

/// Best-effort D1 REST SQL export when the active plugin is D1.
async fn d1_sql_dump(config: &Config) -> Option<Vec<u8>> {
    bookclerk_plugin_host::try_export_d1_sql_dump(config).await
}

/// Snapshot options used before explicit CLI migrate / downgrade.
async fn apply_opts(config: &Config, include_plugin_databases: bool) -> SchemaApplyOptions {
    SchemaApplyOptions {
        snapshot: Some(SchemaSnapshotOpts {
            files_dir: config.paths().files_dir.clone(),
            sqlite_path: sqlite_path_opt(config),
            include_plugin_databases,
            sql_dump: d1_sql_dump(config).await,
        }),
    }
}

/// Prints this binary's schema version and the database's current version.
async fn run_version(config: &Config, format: OutputFormat) -> anyhow::Result<()> {
    let (db, kind, _) = open_unmigrated(config).await?;
    let database = current_schema_version(&db, kind).await?;
    let plan = host_migration_plan();
    let step = plan.iter().find(|s| s.version == database);
    let payload = json!({
        "binary_schema_version": SCHEMA_VERSION,
        "database_schema_version": database,
        "introduced_in": step.map(|s| s.introduced_in).unwrap_or(SCHEMA_V1_INTRODUCED_IN),
        "checksum": step.map(|s| s.checksum()),
        "app_version": env!("CARGO_PKG_VERSION"),
        "reversible": step.map(|s| s.reversible()).unwrap_or(false),
    });
    emit(format, &payload, || {
        println!(
            "binary schema {}  database schema {}  app {}",
            SCHEMA_VERSION,
            database,
            env!("CARGO_PKG_VERSION")
        );
    })
}

/// Writes an in-place snapshot and optionally packages `--path` as `.tar.gz`.
async fn run_snapshot(
    config: &Config,
    format: OutputFormat,
    path: Option<PathBuf>,
    include_plugin_databases: bool,
) -> anyhow::Result<()> {
    let (db, kind, sqlite_path) = open_unmigrated(config).await?;
    let from = current_schema_version(&db, kind).await?;
    let req = SnapshotRequest {
        files_dir: config.paths().files_dir.clone(),
        from_version: from.max(1),
        to_version: from.max(1),
        sqlite_path,
        include_plugin_databases,
        sql_dump: d1_sql_dump(config).await,
    };
    let outcome = snapshot_library(&db, &req)
        .await?
        .ok_or_else(|| anyhow::anyhow!("database is empty; nothing to snapshot"))?;
    let mut archive = None;
    if let Some(dest) = path {
        if is_tar_gz(&dest) {
            archive_snapshot_dir(&outcome.dir, &dest)?;
            archive = Some(dest.display().to_string());
        } else {
            copy_dir_all(&outcome.dir, &dest)?;
            archive = Some(dest.display().to_string());
        }
    }
    let payload = json!({
        "dir": outcome.dir.display().to_string(),
        "schema_version": outcome.manifest.schema_version,
        "kind": outcome.manifest.kind,
        "archive": archive,
    });
    emit(format, &payload, || {
        println!("snapshot {}", outcome.dir.display());
    })
}

/// Restores `library.db` from a snapshot directory or `.tar.gz` archive.
async fn run_restore(config: &Config, format: OutputFormat, from: PathBuf) -> anyhow::Result<()> {
    let (db, _, sqlite_path) = open_unmigrated(config).await?;
    let unpack_root;
    let snapshot_dir = if from.is_file() {
        unpack_root = config.paths().files_dir.join("snapshots").join(format!(
            ".unpack-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.fZ")
        ));
        extract_snapshot_archive(&from, &unpack_root)?;
        unpack_root.clone()
    } else {
        from.clone()
    };
    let manifest = restore_snapshot(&db, &snapshot_dir, sqlite_path.as_deref()).await?;
    if from.is_file() {
        let _ = std::fs::remove_dir_all(&snapshot_dir);
    }
    let payload = json!({
        "schema_version": manifest.schema_version,
        "kind": manifest.kind,
        "from": from.display().to_string(),
    });
    emit(format, &payload, || {
        println!(
            "restored schema {} from {}",
            manifest.schema_version,
            from.display()
        );
    })
}

/// Applies ups or last-reversible downs toward `target`.
async fn run_migrate(
    config: &Config,
    format: OutputFormat,
    target: i64,
    include_plugin_databases: bool,
) -> anyhow::Result<()> {
    let (db, kind, _) = open_unmigrated(config).await?;
    let before = current_schema_version(&db, kind).await?;
    let walk = migrate_host_schema_to(
        &db,
        kind,
        target,
        apply_opts(config, include_plugin_databases).await,
    )
    .await?;
    let after = current_schema_version(&db, kind).await?;
    let payload = json!({
        "from": walk.from,
        "requested_to": walk.requested_to,
        "stopped_at": walk.stopped_at,
        "blocked": walk.blocked,
        "before": before,
        "after": after,
    });
    if walk.blocked || after != target {
        emit(format, &payload, || {
            eprintln!(
                "stopped at schema {} (requested {target}); restore a snapshot if this binary cannot start",
                walk.stopped_at
            );
        })?;
        anyhow::bail!(
            "schema is still at {} (target {target}); restore a snapshot",
            walk.stopped_at
        );
    }
    emit(format, &payload, || {
        println!("schema {} -> {}", walk.from, walk.stopped_at);
    })
}

/// True when `path` looks like a gzipped tar archive.
fn is_tar_gz(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.ends_with(".tar.gz") || name.ends_with(".tgz")
}

/// Recursively copies `src` into `dest`.
fn copy_dir_all(src: &Path, dest: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
