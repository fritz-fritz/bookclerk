//! PostgreSQL engine for the database plugin (SeaORM sqlx-postgres).

use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbErr, Statement};

/// Open Postgres with a host-mediated connection URL (ping only; host applies schema).
///
/// # Errors
///
/// Returns an error when the operation fails.
pub async fn open(url: &str) -> std::result::Result<DatabaseConnection, DbErr> {
    let db = Database::connect(url).await?;
    db.ping().await?;
    tracing::debug!(plugin = "postgres", "opened library database");
    Ok(db)
}

/// Open a dedicated per-binding connection pinned to its own schema.
///
/// Creates the schema when missing and pins the pool `search_path` to it, so
/// unqualified identifiers resolve only inside the binding's schema. The
/// host-side binding policy rejects schema-qualified names, which closes the
/// cross-schema escape.
///
/// # Errors
///
/// Returns an error when the connection, schema creation, or ping fails.
pub async fn open_binding(
    url: &str,
    schema: &str,
) -> std::result::Result<DatabaseConnection, DbErr> {
    if schema.is_empty()
        || !schema
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(DbErr::Custom(format!(
            "invalid binding schema name `{schema}`"
        )));
    }
    // Provision via a plain connection first (search_path-independent DDL).
    let admin = Database::connect(url).await?;
    admin
        .execute_raw(Statement::from_string(
            admin.get_database_backend(),
            format!("CREATE SCHEMA IF NOT EXISTS \"{schema}\""),
        ))
        .await?;
    let mut opt = ConnectOptions::new(url.to_owned());
    opt.set_schema_search_path(schema.to_owned());
    let db = Database::connect(opt).await?;
    db.ping().await?;
    tracing::debug!(plugin = "postgres", schema, "opened binding database");
    Ok(db)
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;

    fn postgres_test_url() -> String {
        let url = std::env::var("BOOKCLERK_TEST_POSTGRES_URL").unwrap_or_else(|_| {
            panic!(
                "BOOKCLERK_TEST_POSTGRES_URL is required to run postgres binding tests \
                 (CI sets BOOKCLERK_REQUIRE_POSTGRES_TESTS=1)"
            )
        });
        assert!(
            !url.trim().is_empty(),
            "BOOKCLERK_TEST_POSTGRES_URL must not be empty"
        );
        url
    }

    #[test]
    fn open_binding_rejects_unsafe_schema_names() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        for bad in ["", "Public", "a.b", "a\"b", "a b", "a;b"] {
            let err = rt
                .block_on(open_binding("postgres://invalid", bad))
                .expect_err("unsafe schema name must fail before connecting");
            assert!(err.to_string().contains("schema name"), "{bad}: {err}");
        }
    }

    #[tokio::test]
    #[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL and a disposable Postgres"]
    async fn postgres_binding_schemas_isolate_same_named_tables() {
        let url = postgres_test_url();
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let schema_a = format!("pb_test_a_{suffix}");
        let schema_b = format!("pb_test_b_{suffix}");
        let a = open_binding(&url, &schema_a).await.expect("binding A");
        let b = open_binding(&url, &schema_b).await.expect("binding B");
        for (db, marker) in [(&a, "alpha"), (&b, "beta")] {
            db.execute_raw(Statement::from_string(
                db.get_database_backend(),
                "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, body TEXT NOT NULL)".to_string(),
            ))
            .await
            .expect("create per-binding table");
            db.execute_raw(Statement::from_string(
                db.get_database_backend(),
                format!("INSERT INTO notes (body) VALUES ('{marker}')"),
            ))
            .await
            .expect("insert per-binding row");
        }
        for (db, marker) in [(&a, "alpha"), (&b, "beta")] {
            let rows = db
                .query_all_raw(Statement::from_string(
                    db.get_database_backend(),
                    "SELECT body FROM notes ORDER BY id".to_string(),
                ))
                .await
                .expect("select per-binding rows");
            assert_eq!(rows.len(), 1, "pinned search_path must isolate schemas");
            let body: String = rows[0].try_get("", "body").expect("body");
            assert_eq!(body, marker);
        }
        // Cleanup so repeated CI runs stay disposable.
        let admin = Database::connect(url.as_str()).await.expect("admin");
        for schema in [&schema_a, &schema_b] {
            admin
                .execute_raw(Statement::from_string(
                    admin.get_database_backend(),
                    format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"),
                ))
                .await
                .expect("drop test schema");
        }
    }
}
