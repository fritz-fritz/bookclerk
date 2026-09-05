//! Adapter-owned snapshot / identity / restore primitives.
//!
//! Called from first-party guests on a **real** engine connection (not the
//! host SeaORM RPC proxy). Library tests may call the same helpers on an
//! in-process sqlite/postgres `DatabaseConnection`.

use bookclerk_plugin_abi::{
    postgres_identity_function_name, DbIdentityHighWater, IsolationReq, SQL_IDENTITY_TABLE,
};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, Statement,
    TransactionTrait,
};

tokio::task_local! {
    static BEGIN_ISOLATION: IsolationReq;
}

/// Isolation the next SeaORM `begin()` should ask the adapter to realize.
#[must_use]
pub fn pending_begin_isolation() -> IsolationReq {
    BEGIN_ISOLATION
        .try_with(|iso| *iso)
        .unwrap_or(IsolationReq::AtomicBatch)
}

/// Begins a consistent capture transaction.
///
/// Sets [`IsolationReq::ConsistentSnapshot`] for RPC proxies, and applies
/// `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ` on a native Postgres
/// connection (the host RPC proxy always reports the canonical sqlite backend).
///
/// # Errors
///
/// Returns when `BEGIN` or snapshot isolation fails.
pub async fn begin_consistent_snapshot(
    db: &DatabaseConnection,
) -> Result<DatabaseTransaction, DbErr> {
    BEGIN_ISOLATION
        .scope(IsolationReq::ConsistentSnapshot, async {
            let txn = db.begin().await?;
            apply_begin_isolation(&txn, IsolationReq::ConsistentSnapshot).await?;
            Ok(txn)
        })
        .await
}

/// Realizes [`IsolationReq`] on an already-open transaction.
///
/// # Errors
///
/// Returns when the engine rejects the isolation statement.
pub async fn apply_begin_isolation<C>(conn: &C, isolation: IsolationReq) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    match isolation {
        IsolationReq::AtomicBatch | IsolationReq::NestedSavepoint => Ok(()),
        IsolationReq::ConsistentSnapshot => {
            if conn.get_database_backend() == DbBackend::Postgres {
                conn.execute_raw(Statement::from_string(
                    DbBackend::Postgres,
                    "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
                ))
                .await?;
            }
            Ok(())
        }
    }
}

/// Identity high-water from `sqlite_sequence` and [`SQL_IDENTITY_TABLE`].
///
/// # Errors
///
/// Returns when a catalog query fails for a reason other than a missing catalog.
pub async fn export_identity<C>(conn: &C) -> Result<Vec<DbIdentityHighWater>, DbErr>
where
    C: ConnectionTrait,
{
    let backend = conn.get_database_backend();
    let mut by_table: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    if backend == DbBackend::Sqlite {
        merge_identity_rows(
            &mut by_table,
            query_optional(
                conn,
                backend,
                "SELECT name, seq FROM sqlite_sequence",
                "name",
                "seq",
                "sqlite_sequence",
            )
            .await?,
        );
    }
    merge_identity_rows(
        &mut by_table,
        query_optional(
            conn,
            backend,
            &format!("SELECT table_name, last FROM {SQL_IDENTITY_TABLE}"),
            "table_name",
            "last",
            SQL_IDENTITY_TABLE,
        )
        .await?,
    );
    Ok(by_table
        .into_iter()
        .map(|(table, last)| DbIdentityHighWater { table, last })
        .collect())
}

/// Writes identity high-water into adapter catalogs.
///
/// # Errors
///
/// Returns when a catalog write fails.
pub async fn import_identity<C>(conn: &C, rows: &[DbIdentityHighWater]) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    let backend = conn.get_database_backend();
    for row in rows {
        if !portable_ident(&row.table) {
            return Err(DbErr::Custom(format!(
                "refusing to import identity for unsafe table `{}`",
                row.table
            )));
        }
        match backend {
            DbBackend::Sqlite => {
                let _ = conn
                    .execute_raw(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "DELETE FROM sqlite_sequence WHERE name = ?",
                        [sea_orm::Value::from(row.table.clone())],
                    ))
                    .await;
                if row.last > 0 {
                    conn.execute_raw(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "INSERT INTO sqlite_sequence(name, seq) VALUES (?, ?)",
                        [
                            sea_orm::Value::from(row.table.clone()),
                            sea_orm::Value::from(row.last),
                        ],
                    ))
                    .await?;
                }
                let _ = upsert_bookclerk_identity(conn, backend, row).await;
            }
            DbBackend::Postgres => {
                upsert_bookclerk_identity(conn, backend, row).await?;
            }
            _ => {
                upsert_bookclerk_identity(conn, backend, row).await?;
            }
        }
    }
    Ok(())
}

/// User-visible base tables in the current schema.
///
/// # Errors
///
/// Returns when the catalog probe fails.
pub async fn list_user_relations<C>(conn: &C) -> Result<Vec<String>, DbErr>
where
    C: ConnectionTrait,
{
    let backend = conn.get_database_backend();
    let sql = if backend == DbBackend::Postgres {
        "SELECT c.relname::text AS name FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = current_schema() AND c.relkind IN ('r', 'p') \
         ORDER BY c.relname"
            .to_string()
    } else {
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name".to_string()
    };
    let rows = conn
        .query_all_raw(Statement::from_string(backend, sql))
        .await?;
    let mut names = Vec::new();
    for row in rows {
        let name = row
            .try_get::<String>("", "name")
            .or_else(|_| row.try_get_by_index::<String>(0))
            .unwrap_or_default();
        if name.is_empty() || is_engine_catalog(&name) {
            continue;
        }
        names.push(name);
    }
    Ok(names)
}

/// SQLite: `PRAGMA defer_foreign_keys = ON` on the restore transaction.
///
/// # Errors
///
/// Returns when the pragma fails.
pub async fn prepare_unit_restore<C>(conn: &C) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    if conn.get_database_backend() == DbBackend::Sqlite {
        conn.execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA defer_foreign_keys = ON",
        ))
        .await?;
    }
    Ok(())
}

/// Drops named relations. PostgreSQL uses `CASCADE` plus identity functions.
///
/// # Errors
///
/// Returns when a drop fails (other than IF EXISTS no-ops).
pub async fn drop_user_relations<C>(conn: &C, names: &[String]) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    let backend = conn.get_database_backend();
    let cascade = if backend == DbBackend::Postgres {
        " CASCADE"
    } else {
        ""
    };
    for name in names.iter().rev() {
        if !portable_ident(name) {
            return Err(DbErr::Custom(format!(
                "refusing to drop unsafe identifier `{name}` during backup restore"
            )));
        }
        conn.execute_raw(Statement::from_string(
            backend,
            format!("DROP TABLE IF EXISTS {name}{cascade}"),
        ))
        .await?;
        if backend == DbBackend::Postgres {
            let fn_name = postgres_identity_function_name(name);
            let _ = conn
                .execute_raw(Statement::from_string(
                    DbBackend::Postgres,
                    format!("DROP FUNCTION IF EXISTS {fn_name}() CASCADE"),
                ))
                .await;
        }
    }
    Ok(())
}

/// SQLite: `PRAGMA foreign_key_check` must be empty before commit.
///
/// # Errors
///
/// Returns when violations remain or the pragma fails.
pub async fn assert_restore_constraints<C>(conn: &C) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    if conn.get_database_backend() != DbBackend::Sqlite {
        return Ok(());
    }
    let rows = conn
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_key_check",
        ))
        .await?;
    if rows.is_empty() {
        return Ok(());
    }
    let table = rows[0]
        .try_get_by_index::<String>(0)
        .unwrap_or_else(|_| "unknown".into());
    Err(DbErr::Custom(format!(
        "restore left foreign-key violations (e.g. table `{table}`)"
    )))
}

fn portable_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && bookclerk_plugin_abi::sql_v1_ident_in_bounds(s)
}

fn is_engine_catalog(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    folded.starts_with("sqlite_") || folded.starts_with("pg_") || folded == "information_schema"
}

fn merge_identity_rows(
    into: &mut std::collections::BTreeMap<String, i64>,
    rows: Vec<(String, i64)>,
) {
    for (table, last) in rows {
        into.entry(table)
            .and_modify(|cur| *cur = (*cur).max(last))
            .or_insert(last);
    }
}

async fn query_optional<C>(
    conn: &C,
    backend: DbBackend,
    sql: &str,
    table_col: &str,
    last_col: &str,
    missing_name: &str,
) -> Result<Vec<(String, i64)>, DbErr>
where
    C: ConnectionTrait,
{
    match conn
        .query_all_raw(Statement::from_string(backend, sql.to_string()))
        .await
    {
        Ok(rows) => {
            let mut out = Vec::new();
            for row in rows {
                let table = row
                    .try_get::<String>("", table_col)
                    .or_else(|_| row.try_get_by_index::<String>(0))
                    .unwrap_or_default();
                let last = row
                    .try_get::<i64>("", last_col)
                    .ok()
                    .or_else(|| row.try_get_by_index::<i64>(1).ok())
                    .or_else(|| row.try_get::<Option<i64>>("", last_col).ok().flatten())
                    .unwrap_or(0);
                if !table.is_empty() {
                    out.push((table, last));
                }
            }
            Ok(out)
        }
        Err(err)
            if err
                .to_string()
                .to_ascii_lowercase()
                .contains(&missing_name.to_ascii_lowercase())
                || bookclerk_plugin_abi::reserved_catalog_relation_missing(
                    &err.to_string(),
                    missing_name,
                ) =>
        {
            Ok(Vec::new())
        }
        Err(err) => Err(err),
    }
}

async fn upsert_bookclerk_identity<C>(
    conn: &C,
    backend: DbBackend,
    row: &DbIdentityHighWater,
) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    let sql = match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {SQL_IDENTITY_TABLE} (table_name, last) VALUES (?, ?) \
             ON CONFLICT (table_name) DO UPDATE SET last = GREATEST({SQL_IDENTITY_TABLE}.last, EXCLUDED.last)"
        ),
        _ => format!("INSERT OR REPLACE INTO {SQL_IDENTITY_TABLE} (table_name, last) VALUES (?, ?)"),
    };
    match conn
        .execute_raw(Statement::from_sql_and_values(
            backend,
            sql,
            [
                sea_orm::Value::from(row.table.clone()),
                sea_orm::Value::from(row.last),
            ],
        ))
        .await
    {
        Ok(_) => Ok(()),
        Err(err)
            if bookclerk_plugin_abi::reserved_catalog_relation_missing(
                &err.to_string(),
                SQL_IDENTITY_TABLE,
            ) =>
        {
            if row.last > 0 && backend != DbBackend::Sqlite {
                return Err(DbErr::Custom(
                    "backup restore cannot apply identity high-water on this adapter \
                     (no bookclerk_identity table)"
                        .into(),
                ));
            }
            Ok(())
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_begin_isolation_defaults_to_atomic_batch() {
        assert_eq!(pending_begin_isolation(), IsolationReq::AtomicBatch);
    }

    #[test]
    fn engine_catalogs_are_skipped() {
        assert!(is_engine_catalog("sqlite_sequence"));
        assert!(is_engine_catalog("sqlite_master"));
        assert!(is_engine_catalog("pg_class"));
        assert!(is_engine_catalog("information_schema"));
        assert!(!is_engine_catalog("books"));
        assert!(!is_engine_catalog("bookclerk_identity"));
    }

    #[test]
    fn portable_ident_rejects_unsafe_names() {
        assert!(portable_ident("accounts"));
        assert!(portable_ident("_tmp"));
        assert!(!portable_ident(""));
        assert!(!portable_ident("sqlite_sequence;drop"));
        assert!(!portable_ident("pg class"));
        assert!(!portable_ident("1books"));
    }
}
