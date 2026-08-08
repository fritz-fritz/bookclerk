//! Guest-side database plugin: holds one SeaORM connection per process.

use bookclerk_config::Config;
use bookclerk_library::{apply_pending_migrations, connect_postgres, connect_sqlite, D1Proxy};
use bookclerk_plugin_sdk::{
    statement_from_dto, upload_file_path, DbConnectParams, ExecResultDto, ProxyRowDto,
    QueryResultDto, StatementDto,
};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend};
use tokio::sync::Mutex;

type Result<T> = std::result::Result<T, String>;

static DB: Mutex<Option<DatabaseConnection>> = Mutex::const_new(None);

pub async fn guest_connect(params: DbConnectParams) -> Result<()> {
    let backend = params.backend.to_ascii_lowercase();
    let conn = match backend.as_str() {
        "sqlite" => {
            let path =
                upload_file_path(params.sqlite_path.as_deref()).map_err(|e| e.to_string())?;
            connect_sqlite(path.as_ref())
                .await
                .map_err(|e| e.to_string())?
        }
        "d1" => {
            let account_id = params
                .account_id
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "d1 connect requires account_id".to_string())?;
            let database_id = params
                .database_id
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "d1 connect requires database_id".to_string())?;
            let token = params
                .d1_api_token
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "d1 connect requires api token from host".to_string())?;
            let api_base = params
                .api_base
                .unwrap_or_else(|| "https://api.cloudflare.com/client/v4".into());
            let proxy = D1Proxy::new(api_base, account_id, database_id, token);
            let db =
                Database::connect_proxy(DbBackend::Sqlite, std::sync::Arc::new(Box::new(proxy)))
                    .await
                    .map_err(|e| e.to_string())?;
            apply_pending_migrations(&db)
                .await
                .map_err(|e| e.to_string())?;
            db
        }
        "postgres" => {
            let url = params
                .postgres_url
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "postgres connect requires url from host".to_string())?;
            let mut config = Config::default();
            config.database.plugin = "postgres".into();
            config.database.postgres.url = Some(url);
            connect_postgres(&config).await.map_err(|e| e.to_string())?
        }
        other => return Err(format!("unsupported database backend `{other}`")),
    };
    *DB.lock().await = Some(conn);
    Ok(())
}

pub async fn guest_ping() -> Result<()> {
    let conn = connection().await?;
    conn.ping().await.map_err(|e| e.to_string())
}

pub async fn guest_query(dto: StatementDto) -> Result<QueryResultDto> {
    let conn = connection().await?;
    let backend = conn.get_database_backend();
    let stmt = statement_from_dto(dto, backend);
    let rows = conn.query_all_raw(stmt).await.map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row_to_dto(&row)?);
    }
    Ok(QueryResultDto { rows: out })
}

pub async fn guest_execute(dto: StatementDto) -> Result<ExecResultDto> {
    let conn = connection().await?;
    let backend = conn.get_database_backend();
    let stmt = statement_from_dto(dto, backend);
    let result = conn.execute_raw(stmt).await.map_err(|e| e.to_string())?;
    Ok(ExecResultDto {
        last_insert_id: result.last_insert_id(),
        rows_affected: result.rows_affected(),
    })
}

async fn connection() -> Result<DatabaseConnection> {
    DB.lock()
        .await
        .clone()
        .ok_or_else(|| "database not connected — call db.connect first".into())
}

fn row_to_dto(row: &sea_orm::QueryResult) -> Result<ProxyRowDto> {
    // `row` always backs onto a `SqliteProxy` connection (see
    // `bookclerk_library::connect_sqlite`), so this is always a `ProxyRow`
    // carrying already-typed `sea_orm::Value`s. Read those directly instead
    // of probing column types with `try_get::<Option<T>>`: on the proxy
    // backend a *type-mismatched* `try_get` returns `Ok(None)` rather than
    // an `Err`, so probing i64/f64/String/Vec<u8> in sequence silently
    // "succeeds" with the wrong (null) value for the first non-numeric,
    // non-null column instead of falling through to the correct branch —
    // this previously broke every non-integer column, most visibly
    // `account_id`, which caused every new account login to fail with
    // "Type Error: Missing value for column 'account_id'" even though the
    // row was written correctly.
    let proxy_row = row
        .try_as_proxy_row()
        .ok_or_else(|| "expected a sea-orm proxy row (SqliteProxy)".to_string())?;
    let values = proxy_row
        .values
        .iter()
        .map(|(column, value)| (column.clone(), bookclerk_plugin_sdk::sea_value_to_json(value)))
        .collect();
    Ok(ProxyRowDto { values })
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use super::row_to_dto;

    /// Regression test for the account-login bug: a text primary-ish column
    /// (`account_id`) and a nullable text column both used to decode as
    /// JSON `null` because `try_get::<Option<i64>>` on the proxy backend
    /// returns `Ok(None)` for a type mismatch instead of `Err`.
    #[tokio::test]
    async fn row_to_dto_preserves_text_and_null_columns() {
        let db = bookclerk_library::connect_sqlite_memory().await.unwrap();
        db.execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO accounts (account_id, marketplace, label, source, created_at, updated_at) \
             VALUES ('alice@example.com', 'us', NULL, 'graphicaudio', '2026-01-01', '2026-01-01')"
                .to_string(),
        ))
        .await
        .unwrap();
        let row = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id, account_id, label FROM accounts".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();

        let dto = row_to_dto(&row).unwrap();
        assert_eq!(
            dto.values.get("account_id"),
            Some(&serde_json::json!("alice@example.com")),
            "text column must round-trip, not decode as null"
        );
        assert_eq!(
            dto.values.get("id"),
            Some(&serde_json::json!(1)),
            "integer column must still decode correctly"
        );
        // A genuine NULL is round-tripped as a typed `$sea_null` marker (see
        // `bookclerk_plugin_sdk::db::sea_value_to_json`), not plain JSON
        // `null` — that's how the host side knows which `sea_orm::Value`
        // variant to reconstruct.
        assert_eq!(
            dto.values.get("label"),
            Some(&serde_json::json!({"$sea_null": "String"})),
            "a genuinely NULL text column must round-trip as a typed null marker"
        );
    }
}

