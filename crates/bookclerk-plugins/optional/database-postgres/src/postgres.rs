//! PostgreSQL engine for the database plugin (SeaORM sqlx-postgres).

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbErr, Statement};

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

/// Rewrites the database name in a Postgres URL, preserving query options.
#[must_use]
pub fn postgres_url_with_database(url: &str, database: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let trimmed = base.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(slash) => {
            let head = &trimmed[..slash];
            match query {
                Some(q) => format!("{head}/{database}?{q}"),
                None => format!("{head}/{database}"),
            }
        }
        None => match query {
            Some(q) => format!("{trimmed}/{database}?{q}"),
            None => format!("{trimmed}/{database}"),
        },
    }
}

/// True when `name` is a safe unquoted Postgres identifier (`[a-z0-9_]+`).
fn binding_database_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Open a dedicated per-binding **database** (not a schema on the library DB).
///
/// Creates the database when missing, then connects to it. Plugin SQL cannot
/// see host library tables in another database, even via `CREATE TABLE AS`
/// or `REFERENCES public.books`.
///
/// # Errors
///
/// Returns an error when the name is unsafe, `CREATE DATABASE` fails (the
/// role needs `CREATEDB`), or the binding connection cannot ping.
pub async fn open_binding(
    url: &str,
    database: &str,
) -> std::result::Result<DatabaseConnection, DbErr> {
    if !binding_database_name_ok(database) {
        return Err(DbErr::Custom(format!(
            "invalid binding database name `{database}`"
        )));
    }
    let admin = Database::connect(url).await?;
    let backend = admin.get_database_backend();
    let existing = admin
        .query_all_raw(Statement::from_string(
            backend,
            format!("SELECT 1 FROM pg_database WHERE datname = '{database}'"),
        ))
        .await?;
    if existing.is_empty() {
        admin
            .execute_raw(Statement::from_string(
                backend,
                format!("CREATE DATABASE {database}"),
            ))
            .await
            .map_err(|err| {
                DbErr::Custom(format!(
                    "could not create isolated plugin database `{database}` \
                     (the Postgres role needs CREATEDB): {err}"
                ))
            })?;
    }
    drop(admin);
    let db = Database::connect(postgres_url_with_database(url, database)).await?;
    db.ping().await?;
    tracing::debug!(plugin = "postgres", database, "opened binding database");
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
    fn open_binding_rejects_unsafe_database_names() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        for bad in ["", "Public", "a.b", "a\"b", "a b", "a;b"] {
            let err = rt
                .block_on(open_binding("postgres://invalid", bad))
                .expect_err("unsafe database name must fail before connecting");
            assert!(err.to_string().contains("database name"), "{bad}: {err}");
        }
    }

    #[test]
    fn postgres_url_with_database_preserves_query() {
        assert_eq!(
            postgres_url_with_database("postgres://h/library?sslmode=require", "pb_echo_db"),
            "postgres://h/pb_echo_db?sslmode=require"
        );
    }

    #[tokio::test]
    #[ignore = "requires BOOKCLERK_TEST_POSTGRES_URL and a disposable Postgres"]
    async fn postgres_binding_databases_are_physically_separate() {
        let url = postgres_test_url();
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let db_a = format!("pb_test_a_{suffix}");
        let db_b = format!("pb_test_b_{suffix}");
        // Truncate to Postgres NAMEDATALEN (63) if the nanos suffix is long.
        let db_a = db_a.chars().take(63).collect::<String>();
        let db_b = db_b.chars().take(63).collect::<String>();
        let a = open_binding(&url, &db_a).await.expect("binding A");
        let b = open_binding(&url, &db_b).await.expect("binding B");
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
            assert_eq!(rows.len(), 1, "each binding database is isolated");
            let body: String = rows[0].try_get("", "body").expect("body");
            assert_eq!(body, marker);
            let host = db
                .query_all_raw(Statement::from_string(
                    db.get_database_backend(),
                    "SELECT to_regclass('public.books') IS NOT NULL AS present".to_string(),
                ))
                .await
                .expect("host table probe");
            let present: bool = host[0].try_get("", "present").unwrap_or(true);
            assert!(
                !present,
                "binding database must not contain the host library catalog"
            );
        }
        drop(a);
        drop(b);
        let admin = Database::connect(url.as_str()).await.expect("admin");
        let backend = admin.get_database_backend();
        for name in [&db_a, &db_b] {
            admin
                .execute_raw(Statement::from_string(
                    backend,
                    format!("DROP DATABASE IF EXISTS {name}"),
                ))
                .await
                .expect("drop test database");
        }
    }
}
