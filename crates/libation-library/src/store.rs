//! SQLite-backed library store.

use std::path::Path;

use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use crate::error::{LibraryError, Result};
use crate::models::{AccountRecord, BookRecord, LiberateStatus};

/// Handle to the Libation library database.
#[derive(Clone, Debug)]
pub struct LibraryStore {
    pool: SqlitePool,
}

impl LibraryStore {
    /// Open (or create) the SQLite database at `path` and run migrations.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    /// In-memory database (tests).
    pub async fn open_in_memory() -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL,
                applied_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        let current: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_migrations")
                .fetch_one(&self.pool)
                .await?;

        if current < 1 {
            sqlx::query(
                r#"
                CREATE TABLE accounts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    account_id TEXT NOT NULL UNIQUE,
                    marketplace TEXT NOT NULL,
                    label TEXT,
                    scan_enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE books (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    asin TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    marketplace TEXT NOT NULL,
                    title TEXT NOT NULL,
                    authors TEXT,
                    narrators TEXT,
                    series TEXT,
                    series_index TEXT,
                    liberate_status TEXT NOT NULL DEFAULT 'not_liberated',
                    storage_key TEXT,
                    error_message TEXT,
                    purchased_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(asin, account_id),
                    FOREIGN KEY(account_id) REFERENCES accounts(account_id) ON DELETE CASCADE
                );

                CREATE INDEX idx_books_status ON books(liberate_status);
                CREATE INDEX idx_books_account ON books(account_id);
                CREATE INDEX idx_books_title ON books(title);
                "#,
            )
            .execute(&self.pool)
            .await?;

            sqlx::query("INSERT INTO schema_migrations (version, applied_at) VALUES (1, ?)")
                .bind(Utc::now().to_rfc3339())
                .execute(&self.pool)
                .await?;

            tracing::info!("applied library schema migration v1");
        }

        Ok(())
    }

    /// Upsert an account.
    pub async fn upsert_account(
        &self,
        account_id: &str,
        marketplace: &str,
        label: Option<&str>,
        scan_enabled: bool,
    ) -> Result<AccountRecord> {
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO accounts (account_id, marketplace, label, scan_enabled, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(account_id) DO UPDATE SET
                marketplace = excluded.marketplace,
                label = excluded.label,
                scan_enabled = excluded.scan_enabled,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(account_id)
        .bind(marketplace)
        .bind(label)
        .bind(i64::from(scan_enabled))
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        self.get_account(account_id)
            .await?
            .ok_or_else(|| LibraryError::NotFound(account_id.into()))
    }

    pub async fn get_account(&self, account_id: &str) -> Result<Option<AccountRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, account_id, marketplace, label, scan_enabled, created_at, updated_at
            FROM accounts WHERE account_id = ?
            "#,
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            let created_at: String = r.get("created_at");
            let updated_at: String = r.get("updated_at");
            AccountRecord {
                id: r.get("id"),
                account_id: r.get("account_id"),
                marketplace: r.get("marketplace"),
                label: r.get("label"),
                scan_enabled: r.get::<i64, _>("scan_enabled") != 0,
                created_at: parse_dt(&created_at),
                updated_at: parse_dt(&updated_at),
            }
        }))
    }

    pub async fn list_accounts(&self) -> Result<Vec<AccountRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, account_id, marketplace, label, scan_enabled, created_at, updated_at
            FROM accounts ORDER BY account_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let created_at: String = r.get("created_at");
                let updated_at: String = r.get("updated_at");
                AccountRecord {
                    id: r.get("id"),
                    account_id: r.get("account_id"),
                    marketplace: r.get("marketplace"),
                    label: r.get("label"),
                    scan_enabled: r.get::<i64, _>("scan_enabled") != 0,
                    created_at: parse_dt(&created_at),
                    updated_at: parse_dt(&updated_at),
                }
            })
            .collect())
    }

    /// Upsert a book from a library sync.
    pub async fn upsert_book(&self, book: &NewBook) -> Result<BookRecord> {
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO books (
                asin, account_id, marketplace, title, authors, narrators, series, series_index,
                liberate_status, purchased_at, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(asin, account_id) DO UPDATE SET
                marketplace = excluded.marketplace,
                title = excluded.title,
                authors = excluded.authors,
                narrators = excluded.narrators,
                series = excluded.series,
                series_index = excluded.series_index,
                purchased_at = excluded.purchased_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&book.asin)
        .bind(&book.account_id)
        .bind(&book.marketplace)
        .bind(&book.title)
        .bind(&book.authors)
        .bind(&book.narrators)
        .bind(&book.series)
        .bind(&book.series_index)
        .bind(LiberateStatus::NotLiberated.as_str())
        .bind(book.purchased_at.map(|d| d.to_rfc3339()))
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        self.get_book(&book.asin, &book.account_id)
            .await?
            .ok_or_else(|| LibraryError::NotFound(book.asin.clone()))
    }

    pub async fn get_book(&self, asin: &str, account_id: &str) -> Result<Option<BookRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, asin, account_id, marketplace, title, authors, narrators, series, series_index,
                   liberate_status, storage_key, error_message, purchased_at, created_at, updated_at
            FROM books WHERE asin = ? AND account_id = ?
            "#,
        )
        .bind(asin)
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(map_book_row))
    }

    pub async fn list_books(&self, account_id: Option<&str>) -> Result<Vec<BookRecord>> {
        let rows = if let Some(account_id) = account_id {
            sqlx::query(
                r#"
                SELECT id, asin, account_id, marketplace, title, authors, narrators, series, series_index,
                       liberate_status, storage_key, error_message, purchased_at, created_at, updated_at
                FROM books WHERE account_id = ? ORDER BY title
                "#,
            )
            .bind(account_id)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, asin, account_id, marketplace, title, authors, narrators, series, series_index,
                       liberate_status, storage_key, error_message, purchased_at, created_at, updated_at
                FROM books ORDER BY title
                "#,
            )
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.into_iter().map(map_book_row).collect())
    }

    pub async fn set_liberate_status(
        &self,
        asin: &str,
        account_id: &str,
        status: LiberateStatus,
        storage_key: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now();
        let result = sqlx::query(
            r#"
            UPDATE books SET
                liberate_status = ?,
                storage_key = COALESCE(?, storage_key),
                error_message = ?,
                updated_at = ?
            WHERE asin = ? AND account_id = ?
            "#,
        )
        .bind(status.as_str())
        .bind(storage_key)
        .bind(error_message)
        .bind(now.to_rfc3339())
        .bind(asin)
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(LibraryError::NotFound(asin.into()));
        }
        Ok(())
    }

    pub async fn count_by_status(&self, status: LiberateStatus) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM books WHERE liberate_status = ?")
            .bind(status.as_str())
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }
}

/// Input for inserting / updating a book from sync.
#[derive(Debug, Clone)]
pub struct NewBook {
    pub asin: String,
    pub account_id: String,
    pub marketplace: String,
    pub title: String,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub purchased_at: Option<chrono::DateTime<Utc>>,
}

fn map_book_row(r: sqlx::sqlite::SqliteRow) -> BookRecord {
    let status_raw: String = r.get("liberate_status");
    let created_at: String = r.get("created_at");
    let updated_at: String = r.get("updated_at");
    BookRecord {
        id: r.get("id"),
        asin: r.get("asin"),
        account_id: r.get("account_id"),
        marketplace: r.get("marketplace"),
        title: r.get("title"),
        authors: r.get("authors"),
        narrators: r.get("narrators"),
        series: r.get("series"),
        series_index: r.get("series_index"),
        liberate_status: LiberateStatus::parse(&status_raw).unwrap_or_default(),
        storage_key: r.get("storage_key"),
        error_message: r.get("error_message"),
        purchased_at: r
            .get::<Option<String>, _>("purchased_at")
            .as_ref()
            .map(|s| parse_dt(s)),
        created_at: parse_dt(&created_at),
        updated_at: parse_dt(&updated_at),
    }
}

fn parse_dt(value: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn account_and_book_roundtrip() {
        let store = LibraryStore::open_in_memory().await.unwrap();
        let acct = store
            .upsert_account("user-1", "us", Some("Main"), true)
            .await
            .unwrap();
        assert_eq!(acct.account_id, "user-1");

        let book = store
            .upsert_book(&NewBook {
                asin: "B00TEST".into(),
                account_id: "user-1".into(),
                marketplace: "us".into(),
                title: "Test Book".into(),
                authors: Some("Author".into()),
                narrators: None,
                series: None,
                series_index: None,
                purchased_at: None,
            })
            .await
            .unwrap();
        assert_eq!(book.title, "Test Book");
        assert_eq!(book.liberate_status, LiberateStatus::NotLiberated);

        store
            .set_liberate_status(
                "B00TEST",
                "user-1",
                LiberateStatus::Liberated,
                Some("Author/Test Book/book.m4b"),
                None,
            )
            .await
            .unwrap();

        let updated = store.get_book("B00TEST", "user-1").await.unwrap().unwrap();
        assert_eq!(updated.liberate_status, LiberateStatus::Liberated);
        assert_eq!(
            updated.storage_key.as_deref(),
            Some("Author/Test Book/book.m4b")
        );
        assert_eq!(
            store
                .count_by_status(LiberateStatus::Liberated)
                .await
                .unwrap(),
            1
        );
    }
}
