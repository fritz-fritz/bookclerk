//! Host-canonical SQL transport.
//!
//! Library production code executes Bookclerk SQL with `?` placeholders on the
//! canonical SeaORM SQLite-shaped backend. That backend is **not** the physical
//! engine: it exists so SeaORM and host helpers never emit `$n`, `GLOB`, or
//! other adapter syntax. Adapters lower at execute.

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, ExecResult, QueryResult, Statement, Value};

/// SeaORM backend used for host-authored canonical SQL (`?` placeholders).
pub const HOST_CANONICAL_BACKEND: DatabaseBackend = DatabaseBackend::Sqlite;

/// Builds a SeaORM statement that keeps canonical `?` placeholders.
#[must_use]
pub fn host_canonical_statement(
    sql: impl Into<String>,
    values: impl IntoIterator<Item = Value>,
) -> Statement {
    Statement::from_sql_and_values(HOST_CANONICAL_BACKEND, sql, values)
}

/// Executes already-canonical host SQL. Never desugars or physically lowers.
///
/// # Errors
///
/// Returns when the connection rejects the statement.
pub async fn execute_host_canonical<C>(
    db: &C,
    sql: &str,
    values: impl IntoIterator<Item = Value>,
) -> Result<ExecResult, DbErr>
where
    C: ConnectionTrait,
{
    db.execute_raw(host_canonical_statement(sql, values)).await
}

/// Queries already-canonical host SQL. Never desugars or physically lowers.
///
/// # Errors
///
/// Returns when the connection rejects the statement.
pub async fn query_host_canonical<C>(
    db: &C,
    sql: &str,
    values: impl IntoIterator<Item = Value>,
) -> Result<Vec<QueryResult>, DbErr>
where
    C: ConnectionTrait,
{
    db.query_all_raw(host_canonical_statement(sql, values))
        .await
}

#[cfg(test)]
mod tests {
    use super::{host_canonical_statement, HOST_CANONICAL_BACKEND};
    use sea_orm::{DatabaseBackend, Statement};

    #[test]
    fn host_statements_keep_question_marks_and_like() {
        let sql = "SELECT id FROM t WHERE title LIKE ? AND a = ?";
        let stmt = host_canonical_statement(sql, Vec::<sea_orm::Value>::new());
        assert_eq!(stmt.db_backend, HOST_CANONICAL_BACKEND);
        assert_eq!(stmt.db_backend, DatabaseBackend::Sqlite);
        assert!(stmt.sql.contains('?'), "{}", stmt.sql);
        assert!(stmt.sql.contains("LIKE"), "{}", stmt.sql);
        assert!(!stmt.sql.contains("GLOB"), "{}", stmt.sql);
        assert!(!stmt.sql.contains("$1"), "{}", stmt.sql);
        let rebuilt = Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, []);
        assert_eq!(rebuilt.sql, sql);
    }
}
