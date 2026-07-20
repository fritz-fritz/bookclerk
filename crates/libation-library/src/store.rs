//! SQLite-backed library store (rusqlite + bundled libsqlite3).

use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{LibraryError, Result};
use crate::migrations;
use crate::models::{AccountRecord, BookRecord, LiberateStatus};

/// Handle to the Libation library database.
///
/// Sync API (SQLite is local and fast). Callers in async contexts may invoke
/// these methods directly; wrap in `spawn_blocking` only for long batches.
#[derive(Clone)]
pub struct LibraryStore {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for LibraryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryStore").finish_non_exhaustive()
    }
}

impl LibraryStore {
    /// Open (or create) the SQLite database at `path` and run migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrations::migrations().to_latest(&mut conn)?;
        tracing::debug!(path = %path.display(), "opened library database");
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// In-memory database (tests).
    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrations::migrations().to_latest(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().map_err(|_| {
            LibraryError::Other(anyhow::anyhow!("library database mutex poisoned"))
        })?;
        f(&conn)
    }

    /// Upsert an account.
    pub fn upsert_account(
        &self,
        account_id: &str,
        marketplace: &str,
        label: Option<&str>,
        scan_enabled: bool,
    ) -> Result<AccountRecord> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO accounts (account_id, marketplace, label, scan_enabled, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(account_id) DO UPDATE SET
                    marketplace = excluded.marketplace,
                    label = excluded.label,
                    scan_enabled = excluded.scan_enabled,
                    updated_at = excluded.updated_at
                "#,
                params![
                    account_id,
                    marketplace,
                    label,
                    i64::from(scan_enabled),
                    now,
                    now
                ],
            )?;
            Ok(())
        })?;
        self.get_account(account_id)?
            .ok_or_else(|| LibraryError::NotFound(account_id.into()))
    }

    pub fn get_account(&self, account_id: &str) -> Result<Option<AccountRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                r#"
                SELECT id, account_id, marketplace, label, scan_enabled, created_at, updated_at
                FROM accounts WHERE account_id = ?1
                "#,
                params![account_id],
                map_account_row,
            )
            .optional()
            .map_err(LibraryError::from)
        })
    }

    pub fn list_accounts(&self) -> Result<Vec<AccountRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, account_id, marketplace, label, scan_enabled, created_at, updated_at
                FROM accounts ORDER BY account_id
                "#,
            )?;
            let rows = stmt
                .query_map([], map_account_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Upsert a book from a library sync.
    pub fn upsert_book(&self, book: &NewBook) -> Result<BookRecord> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO books (
                    asin, account_id, marketplace, title, authors, narrators, series, series_index,
                    liberate_status, purchased_at, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
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
                params![
                    book.asin,
                    book.account_id,
                    book.marketplace,
                    book.title,
                    book.authors,
                    book.narrators,
                    book.series,
                    book.series_index,
                    LiberateStatus::NotLiberated.as_str(),
                    book.purchased_at.map(|d| d.to_rfc3339()),
                    now,
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.get_book(&book.asin, &book.account_id)?
            .ok_or_else(|| LibraryError::NotFound(book.asin.clone()))
    }

    pub fn get_book(&self, asin: &str, account_id: &str) -> Result<Option<BookRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                r#"
                SELECT id, asin, account_id, marketplace, title, authors, narrators, series, series_index,
                       liberate_status, storage_key, error_message, purchased_at, created_at, updated_at
                FROM books WHERE asin = ?1 AND account_id = ?2
                "#,
                params![asin, account_id],
                map_book_row,
            )
            .optional()
            .map_err(LibraryError::from)
        })
    }

    pub fn list_books(&self, account_id: Option<&str>) -> Result<Vec<BookRecord>> {
        self.with_conn(|conn| {
            if let Some(account_id) = account_id {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT id, asin, account_id, marketplace, title, authors, narrators, series, series_index,
                           liberate_status, storage_key, error_message, purchased_at, created_at, updated_at
                    FROM books WHERE account_id = ?1 ORDER BY title
                    "#,
                )?;
                let rows = stmt
                    .query_map(params![account_id], map_book_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            } else {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT id, asin, account_id, marketplace, title, authors, narrators, series, series_index,
                           liberate_status, storage_key, error_message, purchased_at, created_at, updated_at
                    FROM books ORDER BY title
                    "#,
                )?;
                let rows = stmt
                    .query_map([], map_book_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            }
        })
    }

    pub fn set_liberate_status(
        &self,
        asin: &str,
        account_id: &str,
        status: LiberateStatus,
        storage_key: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let rows = self.with_conn(|conn| {
            let n = conn.execute(
                r#"
                UPDATE books SET
                    liberate_status = ?1,
                    storage_key = COALESCE(?2, storage_key),
                    error_message = ?3,
                    updated_at = ?4
                WHERE asin = ?5 AND account_id = ?6
                "#,
                params![
                    status.as_str(),
                    storage_key,
                    error_message,
                    now,
                    asin,
                    account_id
                ],
            )?;
            Ok(n)
        })?;
        if rows == 0 {
            return Err(LibraryError::NotFound(asin.into()));
        }
        Ok(())
    }

    pub fn count_by_status(&self, status: LiberateStatus) -> Result<i64> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM books WHERE liberate_status = ?1",
                params![status.as_str()],
                |row| row.get(0),
            )
            .map_err(LibraryError::from)
        })
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

fn map_account_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AccountRecord> {
    let created_at: String = r.get("created_at")?;
    let updated_at: String = r.get("updated_at")?;
    Ok(AccountRecord {
        id: r.get("id")?,
        account_id: r.get("account_id")?,
        marketplace: r.get("marketplace")?,
        label: r.get("label")?,
        scan_enabled: r.get::<_, i64>("scan_enabled")? != 0,
        created_at: parse_dt(&created_at),
        updated_at: parse_dt(&updated_at),
    })
}

fn map_book_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<BookRecord> {
    let status_raw: String = r.get("liberate_status")?;
    let created_at: String = r.get("created_at")?;
    let updated_at: String = r.get("updated_at")?;
    let purchased_at: Option<String> = r.get("purchased_at")?;
    Ok(BookRecord {
        id: r.get("id")?,
        asin: r.get("asin")?,
        account_id: r.get("account_id")?,
        marketplace: r.get("marketplace")?,
        title: r.get("title")?,
        authors: r.get("authors")?,
        narrators: r.get("narrators")?,
        series: r.get("series")?,
        series_index: r.get("series_index")?,
        liberate_status: LiberateStatus::parse(&status_raw).unwrap_or_default(),
        storage_key: r.get("storage_key")?,
        error_message: r.get("error_message")?,
        purchased_at: purchased_at.as_deref().map(parse_dt),
        created_at: parse_dt(&created_at),
        updated_at: parse_dt(&updated_at),
    })
}

fn parse_dt(value: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_and_book_roundtrip() {
        let store = LibraryStore::open_in_memory().unwrap();
        let acct = store
            .upsert_account("user-1", "us", Some("Main"), true)
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
            .unwrap();

        let updated = store.get_book("B00TEST", "user-1").unwrap().unwrap();
        assert_eq!(updated.liberate_status, LiberateStatus::Liberated);
        assert_eq!(
            updated.storage_key.as_deref(),
            Some("Author/Test Book/book.m4b")
        );
        assert_eq!(store.count_by_status(LiberateStatus::Liberated).unwrap(), 1);
    }
}
