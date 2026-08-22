//! PostgreSQL engine for the database plugin (SeaORM sqlx-postgres).

use sea_orm::{Database, DatabaseConnection, DbErr};

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
