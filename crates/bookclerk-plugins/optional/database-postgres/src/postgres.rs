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
