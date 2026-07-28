//! SeaORM-backed library store.
//!
//! Every backend is a [`DatabaseConnection`] proxy (local rusqlite `sqlite` or
//! Cloudflare `d1`). The public API stays synchronous by driving each query
//! with [`block_on_db`]; SQL uses `?N` positional placeholders and reads pull
//! typed values out of the proxy [`Row`] (which normalises the proxy quirks:
//! integers arrive as `BigInt`, NULLs as `String(None)`).

use std::collections::BTreeMap;
use std::path::Path;

use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, ExecResult, QueryResult, Statement, Value,
};
use uuid::Uuid;

use crate::db::{block_on_db, connect_from_config, connect_sqlite, connect_sqlite_memory};
use crate::error::{LibraryError, Result};
use crate::models::{
    AccountRecord, AcquireStatus, BookRecord, GlobalQueueEntry, ListeningProgressRecord,
    RequestStatus, TitleRequestRecord, UserPreferences, WorkRecord,
};

const BOOK_SELECT: &str = r#"
    SELECT id, uuid, source, account_id, product_id, asin, isbn, marketplace, title, authors,
           narrators, series, series_index, series_asin, acquire_status, storage_key,
           error_message, purchased_at, tags, rating_overall, rating_performance, rating_story,
           is_finished, pdf_status, pdf_storage_key, publisher, length_minutes, is_abridged,
           content_kind, categories, subtitle, published_at, description, language, cover_url,
           subjects, enrich_source, enrich_confidence, enrich_updated_at, created_at, updated_at
    FROM books
"#;

const ACCOUNT_SELECT: &str = r#"
    SELECT id, account_id, marketplace, label, scan_enabled, source,
           COALESCE(connection_status, 'active') AS connection_status,
           created_at, updated_at
    FROM accounts
"#;

/// Handle to the Bookclerk library database.
///
/// Sync API over a SeaORM [`DatabaseConnection`] proxy. Each method blocks on
/// the underlying async query via [`block_on_db`]; callers already inside a
/// Tokio runtime are handled transparently. `DatabaseConnection` is cheaply
/// cloneable (shared connection), so `LibraryStore` is `Clone`.
#[derive(Clone)]
pub struct LibraryStore {
    db: DatabaseConnection,
}

impl std::fmt::Debug for LibraryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryStore").finish_non_exhaustive()
    }
}

impl LibraryStore {
    /// Open (or create) the SQLite database at `path` and run migrations.
    pub fn open(path: &Path) -> Result<Self> {
        let db = block_on_db(connect_sqlite(path))?;
        Ok(Self { db })
    }

    /// Open the library using `[database]` plugin settings.
    ///
    /// Works for `sqlite`, Cloudflare `d1`, and `postgres`; the store query API
    /// is identical because every backend is a SeaORM [`DatabaseConnection`].
    pub fn open_from_config(config: &bookclerk_config::Config) -> Result<Self> {
        let db = block_on_db(connect_from_config(config))?;
        Ok(Self { db })
    }

    /// In-memory database (tests).
    pub fn open_in_memory() -> Result<Self> {
        let db = block_on_db(connect_sqlite_memory())?;
        Ok(Self { db })
    }

    /// Wrap an already-opened SeaORM connection.
    #[must_use]
    pub fn from_connection(db: DatabaseConnection) -> Self {
        Self { db }
    }

    fn backend(&self) -> DbBackend {
        self.db.get_database_backend()
    }

    /// Execute a write statement, returning the raw [`ExecResult`].
    fn exec(&self, sql: &str, values: Vec<Value>) -> Result<ExecResult> {
        let stmt = Statement::from_sql_and_values(self.backend(), sql, values);
        block_on_db(self.db.execute_raw(stmt)).map_err(LibraryError::Orm)
    }

    /// Number of rows affected by a write statement.
    fn exec_affected(&self, sql: &str, values: Vec<Value>) -> Result<u64> {
        Ok(self.exec(sql, values)?.rows_affected())
    }

    /// Fetch at most one row.
    fn query_one_row(&self, sql: &str, values: Vec<Value>) -> Result<Option<Row>> {
        let stmt = Statement::from_sql_and_values(self.backend(), sql, values);
        let out = block_on_db(self.db.query_one_raw(stmt)).map_err(LibraryError::Orm)?;
        Ok(out.map(|qr| Row::from_query(&qr)))
    }

    /// Fetch all matching rows.
    fn query_all_rows(&self, sql: &str, values: Vec<Value>) -> Result<Vec<Row>> {
        let stmt = Statement::from_sql_and_values(self.backend(), sql, values);
        let out = block_on_db(self.db.query_all_raw(stmt)).map_err(LibraryError::Orm)?;
        Ok(out.iter().map(Row::from_query).collect())
    }

    /// Whether a query returns at least one row (existence probes).
    fn exists(&self, sql: &str, values: Vec<Value>) -> Result<bool> {
        Ok(self.query_one_row(sql, values)?.is_some())
    }

    /// Upsert an account (updates `scan_enabled` on conflict). Defaults `source` to `"audible"`.
    pub fn upsert_account(
        &self,
        account_id: &str,
        marketplace: &str,
        label: Option<&str>,
        scan_enabled: bool,
    ) -> Result<AccountRecord> {
        self.upsert_account_with_source(account_id, marketplace, label, scan_enabled, "audible")
    }

    /// Upsert an account with an explicit catalog `source` (`audible`, `libro`, …).
    pub fn upsert_account_with_source(
        &self,
        account_id: &str,
        marketplace: &str,
        label: Option<&str>,
        scan_enabled: bool,
        source: &str,
    ) -> Result<AccountRecord> {
        self.upsert_account_inner(account_id, marketplace, label, scan_enabled, source, true)
    }

    /// Ensure an account row exists for a scan without overwriting `scan_enabled`.
    /// Defaults `source` to `"audible"`.
    pub fn ensure_account(
        &self,
        account_id: &str,
        marketplace: &str,
        label: Option<&str>,
    ) -> Result<AccountRecord> {
        self.ensure_account_with_source(account_id, marketplace, label, "audible")
    }

    /// Ensure an account row exists with an explicit `source`, without overwriting `scan_enabled`.
    pub fn ensure_account_with_source(
        &self,
        account_id: &str,
        marketplace: &str,
        label: Option<&str>,
        source: &str,
    ) -> Result<AccountRecord> {
        self.upsert_account_inner(account_id, marketplace, label, true, source, false)
    }

    fn upsert_account_inner(
        &self,
        account_id: &str,
        marketplace: &str,
        label: Option<&str>,
        scan_enabled: bool,
        source: &str,
        update_scan_enabled: bool,
    ) -> Result<AccountRecord> {
        let now = Utc::now().to_rfc3339();
        // Reject collisions when the same account_id is claimed by another source.
        let existing_source = self
            .query_one_row(
                "SELECT source FROM accounts WHERE account_id = ?1",
                vec![account_id.into()],
            )?
            .map(|r| r.string_req("source"))
            .transpose()?;
        if let Some(existing) = existing_source.as_deref() {
            if existing != source {
                return Err(LibraryError::Other(anyhow::anyhow!(
                    "account_id `{account_id}` already exists for source `{existing}`; \
                     cannot claim it for source `{source}`"
                )));
            }
        }

        let sql = if update_scan_enabled {
            r#"
            INSERT INTO accounts (account_id, marketplace, label, scan_enabled, source, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(account_id) DO UPDATE SET
                marketplace = excluded.marketplace,
                label = COALESCE(excluded.label, accounts.label),
                scan_enabled = excluded.scan_enabled,
                updated_at = excluded.updated_at
            "#
        } else {
            r#"
            INSERT INTO accounts (account_id, marketplace, label, scan_enabled, source, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(account_id) DO UPDATE SET
                marketplace = excluded.marketplace,
                label = COALESCE(excluded.label, accounts.label),
                updated_at = excluded.updated_at
            "#
        };
        self.exec(
            sql,
            vec![
                account_id.into(),
                marketplace.into(),
                label.into(),
                i64::from(scan_enabled).into(),
                source.into(),
                now.clone().into(),
                now.into(),
            ],
        )?;
        self.get_account(account_id)?
            .ok_or_else(|| LibraryError::NotFound(account_id.into()))
    }

    /// Remap `from` → `to` account ids (books + account row).
    ///
    /// Used when classic Libation stored an email `AccountId` and a later
    /// login/scan discovers Audible `customer_id`.
    pub fn remap_account_id(&self, from: &str, to: &str) -> Result<()> {
        if from == to {
            return Ok(());
        }
        let from_exists = self.exists(
            "SELECT 1 FROM accounts WHERE account_id = ?1",
            vec![from.into()],
        )?;
        if !from_exists {
            return Ok(());
        }

        let to_exists = self.exists(
            "SELECT 1 FROM accounts WHERE account_id = ?1",
            vec![to.into()],
        )?;

        if !to_exists {
            // Copy account row under the new id, then move books, then drop old.
            self.exec(
                r#"
                INSERT INTO accounts (account_id, marketplace, label, scan_enabled, source, connection_status, created_at, updated_at)
                SELECT ?1, marketplace, label, scan_enabled, source, COALESCE(connection_status, 'active'), created_at, ?2
                FROM accounts WHERE account_id = ?3
                "#,
                vec![to.into(), Utc::now().to_rfc3339().into(), from.into()],
            )?;
        } else {
            // Prefer label from the old row when the canonical row has none.
            self.exec(
                r#"
                UPDATE accounts SET
                    label = COALESCE(accounts.label, (SELECT label FROM accounts WHERE account_id = ?1)),
                    updated_at = ?2
                WHERE account_id = ?3
                "#,
                vec![from.into(), Utc::now().to_rfc3339().into(), to.into()],
            )?;
        }

        // Move books that do not already exist under `to` (same source + product_id).
        self.exec(
            r#"
            UPDATE books SET account_id = ?1, updated_at = ?2
            WHERE account_id = ?3
              AND NOT EXISTS (
                  SELECT 1 FROM books b2
                  WHERE b2.account_id = ?1
                    AND b2.source = books.source
                    AND b2.product_id = books.product_id
              )
            "#,
            vec![to.into(), Utc::now().to_rfc3339().into(), from.into()],
        )?;
        // Drop duplicate product rows left on the old id.
        self.exec("DELETE FROM books WHERE account_id = ?1", vec![from.into()])?;
        self.exec(
            "DELETE FROM accounts WHERE account_id = ?1",
            vec![from.into()],
        )?;
        Ok(())
    }

    /// Remap any existing alias account ids onto `canonical_id`, then upsert.
    pub fn reconcile_account_id(
        &self,
        canonical_id: &str,
        aliases: &[&str],
        marketplace: &str,
        label: Option<&str>,
        scan_enabled: bool,
    ) -> Result<AccountRecord> {
        for alias in aliases {
            if *alias != canonical_id {
                self.remap_account_id(alias, canonical_id)?;
            }
        }
        self.upsert_account(canonical_id, marketplace, label, scan_enabled)
    }

    pub fn get_account(&self, account_id: &str) -> Result<Option<AccountRecord>> {
        self.query_one_row(
            &format!("{ACCOUNT_SELECT} WHERE account_id = ?1"),
            vec![account_id.into()],
        )?
        .map(|r| map_account_row(&r))
        .transpose()
    }

    pub fn list_accounts(&self) -> Result<Vec<AccountRecord>> {
        self.query_all_rows(&format!("{ACCOUNT_SELECT} ORDER BY account_id"), vec![])?
            .iter()
            .map(map_account_row)
            .collect()
    }

    /// Resolve an account row by id or nickname (`label`), case-insensitive.
    pub fn find_account(&self, identifier: &str) -> Result<Option<AccountRecord>> {
        let needle = identifier.to_ascii_lowercase();
        Ok(self.list_accounts()?.into_iter().find(|a| {
            a.account_id.eq_ignore_ascii_case(identifier)
                || a.label
                    .as_ref()
                    .is_some_and(|l| l.eq_ignore_ascii_case(identifier))
                || a.account_id.to_ascii_lowercase() == needle
        }))
    }

    /// Toggle whether an account is included in automatic library scans.
    pub fn set_scan_enabled(&self, account_id: &str, scan_enabled: bool) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let updated = self.exec_affected(
            "UPDATE accounts SET scan_enabled = ?1, updated_at = ?2 WHERE account_id = ?3",
            vec![
                i64::from(scan_enabled).into(),
                now.into(),
                account_id.into(),
            ],
        )?;
        if updated == 0 {
            return Err(LibraryError::NotFound(account_id.into()));
        }
        Ok(())
    }

    /// Mark bookstore credentials active again (after reconnect).
    pub fn mark_connection_active(&self, account_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let updated = self.exec_affected(
            r#"
            UPDATE accounts
            SET connection_status = 'active',
                scan_enabled = 1,
                updated_at = ?1
            WHERE account_id = ?2
            "#,
            vec![now.into(), account_id.into()],
        )?;
        if updated == 0 {
            return Err(LibraryError::NotFound(account_id.into()));
        }
        Ok(())
    }

    /// Mark bookstore credentials revoked without deleting the account or books.
    pub fn revoke_credentials(&self, account_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let updated = self.exec_affected(
            r#"
            UPDATE accounts
            SET scan_enabled = 0,
                connection_status = 'revoked',
                updated_at = ?1
            WHERE account_id = ?2
            "#,
            vec![now.into(), account_id.into()],
        )?;
        if updated == 0 {
            return Err(LibraryError::NotFound(account_id.into()));
        }
        Ok(())
    }

    /// Create or fetch a portal identity for `(provider, external_user_id)`.
    pub fn upsert_portal_identity(
        &self,
        provider: &str,
        external_user_id: &str,
        label: Option<&str>,
    ) -> Result<crate::models::PortalIdentity> {
        let now = Utc::now().to_rfc3339();
        self.exec(
            r#"
            INSERT INTO portal_identities (provider, external_user_id, label, created_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(provider, external_user_id) DO UPDATE SET
                label = COALESCE(excluded.label, portal_identities.label)
            "#,
            vec![
                provider.into(),
                external_user_id.into(),
                label.into(),
                now.into(),
            ],
        )?;
        self.get_portal_identity(provider, external_user_id)?
            .ok_or_else(|| LibraryError::NotFound(format!("{provider}:{external_user_id}")))
    }

    /// Look up a portal identity.
    pub fn get_portal_identity(
        &self,
        provider: &str,
        external_user_id: &str,
    ) -> Result<Option<crate::models::PortalIdentity>> {
        self.query_one_row(
            r#"
            SELECT id, provider, external_user_id, label, created_at
            FROM portal_identities
            WHERE provider = ?1 AND external_user_id = ?2
            "#,
            vec![provider.into(), external_user_id.into()],
        )?
        .map(|r| map_portal_identity_row(&r))
        .transpose()
    }

    /// Look up portal identity by row id.
    pub fn get_portal_identity_by_id(
        &self,
        id: i64,
    ) -> Result<Option<crate::models::PortalIdentity>> {
        self.query_one_row(
            r#"
            SELECT id, provider, external_user_id, label, created_at
            FROM portal_identities WHERE id = ?1
            "#,
            vec![id.into()],
        )?
        .map(|r| map_portal_identity_row(&r))
        .transpose()
    }

    /// Insert a claim ticket (store only the hash).
    pub fn insert_claim_ticket(
        &self,
        token_hash: &str,
        identity_id: Option<i64>,
        expires_at: chrono::DateTime<Utc>,
        created_by: &str,
    ) -> Result<crate::models::ClaimTicketRecord> {
        let now = Utc::now().to_rfc3339();
        let expires = expires_at.to_rfc3339();
        self.exec(
            r#"
            INSERT INTO claim_tickets
                (token_hash, identity_id, expires_at, redeemed_at, created_by, created_at)
            VALUES (?1, ?2, ?3, NULL, ?4, ?5)
            "#,
            vec![
                token_hash.into(),
                identity_id.into(),
                expires.into(),
                created_by.into(),
                now.into(),
            ],
        )?;
        self.get_claim_ticket_by_hash(token_hash)?
            .ok_or_else(|| LibraryError::NotFound(token_hash.into()))
    }

    pub fn get_claim_ticket_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<crate::models::ClaimTicketRecord>> {
        self.query_one_row(
            r#"
            SELECT id, token_hash, identity_id, expires_at, redeemed_at, created_by, created_at
            FROM claim_tickets WHERE token_hash = ?1
            "#,
            vec![token_hash.into()],
        )?
        .map(|r| map_claim_ticket_row(&r))
        .transpose()
    }

    /// List unredeemed, unexpired claim tickets (newest first).
    pub fn list_open_claim_tickets(&self) -> Result<Vec<crate::models::ClaimTicketRecord>> {
        let now = Utc::now().to_rfc3339();
        self.query_all_rows(
            r#"
            SELECT id, token_hash, identity_id, expires_at, redeemed_at, created_by, created_at
            FROM claim_tickets
            WHERE redeemed_at IS NULL AND expires_at > ?1
            ORDER BY id DESC
            "#,
            vec![now.into()],
        )?
        .iter()
        .map(map_claim_ticket_row)
        .collect()
    }

    /// Mark a claim ticket redeemed.
    pub fn redeem_claim_ticket(
        &self,
        token_hash: &str,
    ) -> Result<crate::models::ClaimTicketRecord> {
        let now = Utc::now().to_rfc3339();
        let updated = self.exec_affected(
            r#"
            UPDATE claim_tickets
            SET redeemed_at = ?1
            WHERE token_hash = ?2
              AND redeemed_at IS NULL
              AND expires_at > ?1
            "#,
            vec![now.into(), token_hash.into()],
        )?;
        if updated == 0 {
            return Err(LibraryError::Other(anyhow::anyhow!(
                "claim ticket invalid, expired, or already redeemed"
            )));
        }
        self.get_claim_ticket_by_hash(token_hash)?
            .ok_or_else(|| LibraryError::NotFound(token_hash.into()))
    }

    /// Create a portal session (hash only).
    pub fn insert_portal_session(
        &self,
        token_hash: &str,
        identity_id: i64,
        expires_at: chrono::DateTime<Utc>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.exec(
            r#"
            INSERT INTO portal_sessions (token_hash, identity_id, expires_at, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            vec![
                token_hash.into(),
                identity_id.into(),
                expires_at.to_rfc3339().into(),
                now.into(),
            ],
        )?;
        Ok(())
    }

    /// Resolve a valid portal session to its identity.
    pub fn get_portal_session_identity(
        &self,
        token_hash: &str,
    ) -> Result<Option<crate::models::PortalIdentity>> {
        self.query_one_row(
            r#"
            SELECT i.id, i.provider, i.external_user_id, i.label, i.created_at
            FROM portal_sessions s
            JOIN portal_identities i ON i.id = s.identity_id
            WHERE s.token_hash = ?1 AND s.expires_at > ?2
            "#,
            vec![token_hash.into(), Utc::now().to_rfc3339().into()],
        )?
        .map(|r| map_portal_identity_row(&r))
        .transpose()
    }

    /// Link a bookstore account to a portal identity.
    pub fn link_account(
        &self,
        identity_id: i64,
        account_id: &str,
        source: &str,
    ) -> Result<crate::models::AccountLinkRecord> {
        let now = Utc::now().to_rfc3339();
        self.exec(
            r#"
            INSERT INTO account_links (identity_id, account_id, source, created_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(identity_id, account_id) DO NOTHING
            "#,
            vec![
                identity_id.into(),
                account_id.into(),
                source.into(),
                now.into(),
            ],
        )?;
        self.list_account_links(identity_id)?
            .into_iter()
            .find(|l| l.account_id == account_id)
            .ok_or_else(|| LibraryError::NotFound(account_id.into()))
    }

    pub fn list_account_links(
        &self,
        identity_id: i64,
    ) -> Result<Vec<crate::models::AccountLinkRecord>> {
        self.query_all_rows(
            r#"
            SELECT id, identity_id, account_id, source, created_at
            FROM account_links WHERE identity_id = ?1
            ORDER BY id
            "#,
            vec![identity_id.into()],
        )?
        .iter()
        .map(map_account_link_row)
        .collect()
    }

    /// Remove an account link row (does not delete the account).
    pub fn unlink_account(&self, identity_id: i64, account_id: &str) -> Result<()> {
        let updated = self.exec_affected(
            "DELETE FROM account_links WHERE identity_id = ?1 AND account_id = ?2",
            vec![identity_id.into(), account_id.into()],
        )?;
        if updated == 0 {
            return Err(LibraryError::NotFound(account_id.into()));
        }
        Ok(())
    }

    /// Upsert a book from a library sync.
    pub fn upsert_book(&self, book: &NewBook) -> Result<BookRecord> {
        let now = Utc::now().to_rfc3339();
        let uuid = book
            .uuid
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        self.exec(
            r#"
                INSERT INTO books (
                    uuid, source, account_id, product_id, asin, isbn, marketplace, title,
                    authors, narrators, series, series_index, series_asin, acquire_status,
                    purchased_at, publisher, length_minutes, is_abridged, content_kind,
                    categories, subtitle, published_at, created_at, updated_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                    ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24
                )
                ON CONFLICT(source, account_id, product_id) DO UPDATE SET
                    asin = COALESCE(excluded.asin, books.asin),
                    isbn = COALESCE(excluded.isbn, books.isbn),
                    marketplace = excluded.marketplace,
                    -- When the existing row already has an Audible ASIN (native or
                    -- enriched) and the incoming row does not, keep catalog fields
                    -- so a Libro rescan does not wipe Audible enrichment.
                    title = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN excluded.title ELSE books.title END,
                    authors = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN excluded.authors ELSE books.authors END,
                    narrators = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN excluded.narrators ELSE books.narrators END,
                    series = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN excluded.series ELSE books.series END,
                    series_index = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN excluded.series_index ELSE books.series_index END,
                    series_asin = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN COALESCE(excluded.series_asin, books.series_asin)
                        ELSE books.series_asin END,
                    purchased_at = excluded.purchased_at,
                    publisher = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN COALESCE(excluded.publisher, books.publisher)
                        ELSE books.publisher END,
                    length_minutes = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN COALESCE(excluded.length_minutes, books.length_minutes)
                        ELSE books.length_minutes END,
                    is_abridged = excluded.is_abridged,
                    content_kind = excluded.content_kind,
                    categories = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN COALESCE(excluded.categories, books.categories)
                        ELSE books.categories END,
                    subtitle = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN COALESCE(excluded.subtitle, books.subtitle)
                        ELSE books.subtitle END,
                    published_at = CASE
                        WHEN excluded.asin IS NOT NULL OR books.asin IS NULL
                        THEN COALESCE(excluded.published_at, books.published_at)
                        ELSE books.published_at END,
                    updated_at = excluded.updated_at
                "#,
            vec![
                uuid.into(),
                book.source.clone().into(),
                book.account_id.clone().into(),
                book.product_id.clone().into(),
                book.asin.clone().into(),
                book.isbn.clone().into(),
                book.marketplace.clone().into(),
                book.title.clone().into(),
                book.authors.clone().into(),
                book.narrators.clone().into(),
                book.series.clone().into(),
                book.series_index.clone().into(),
                book.series_asin.clone().into(),
                AcquireStatus::NotAcquired.as_str().into(),
                book.purchased_at.map(|d| d.to_rfc3339()).into(),
                book.publisher.clone().into(),
                book.length_minutes.into(),
                i64::from(book.is_abridged).into(),
                book.content_kind.clone().into(),
                book.categories.clone().into(),
                book.subtitle.clone().into(),
                book.published_at.map(|d| d.to_rfc3339()).into(),
                now.clone().into(),
                now.into(),
            ],
        )?;
        self.get_book(&book.product_id, &book.account_id)?
            .ok_or_else(|| LibraryError::NotFound(book.product_id.clone()))
    }

    /// Look up a book by its public `uuid`.
    pub fn get_book_by_uuid(&self, uuid: &str) -> Result<Option<BookRecord>> {
        self.query_one_row(&format!("{BOOK_SELECT} WHERE uuid = ?1"), vec![uuid.into()])?
            .map(|r| map_book_row(&r))
            .transpose()
    }

    /// Look up a book by uuid, product_id, asin, or isbn for the given account.
    ///
    /// Prefer exact uuid / product_id matches. When only ISBN matches multiple
    /// sources under one account, prefer product_id/uuid exactness, then
    /// deterministic `ORDER BY source`.
    pub fn get_book(&self, title_id: &str, account_id: &str) -> Result<Option<BookRecord>> {
        let sql = format!(
            "{BOOK_SELECT}
            WHERE account_id = ?2
              AND (uuid = ?1 OR product_id = ?1 OR asin = ?1 OR isbn = ?1)
            ORDER BY
              CASE
                WHEN uuid = ?1 THEN 0
                WHEN product_id = ?1 THEN 1
                WHEN asin = ?1 THEN 2
                ELSE 3
              END,
              source
            LIMIT 1"
        );
        self.query_one_row(&sql, vec![title_id.into(), account_id.into()])?
            .map(|r| map_book_row(&r))
            .transpose()
    }

    /// All ownership rows sharing an ISBN (cross-account / cross-store enrichment).
    pub fn find_books_by_isbn(&self, isbn: &str) -> Result<Vec<BookRecord>> {
        let sql = format!("{BOOK_SELECT} WHERE isbn = ?1 ORDER BY source, account_id");
        self.query_all_rows(&sql, vec![isbn.into()])?
            .iter()
            .map(map_book_row)
            .collect()
    }

    pub fn list_books(&self, account_id: Option<&str>) -> Result<Vec<BookRecord>> {
        let rows = if let Some(account_id) = account_id {
            let sql = format!("{BOOK_SELECT} WHERE account_id = ?1 ORDER BY title");
            self.query_all_rows(&sql, vec![account_id.into()])?
        } else {
            let sql = format!("{BOOK_SELECT} ORDER BY title");
            self.query_all_rows(&sql, vec![])?
        };
        rows.iter().map(map_book_row).collect()
    }

    /// Resolve `title_id` to the stored public uuid, or error if missing.
    fn resolve_uuid(&self, title_id: &str, account_id: &str) -> Result<String> {
        self.get_book(title_id, account_id)?
            .map(|b| b.uuid)
            .ok_or_else(|| LibraryError::NotFound(title_id.into()))
    }

    /// Update user-defined fields (tags, ratings, finished) without touching scan metadata.
    pub fn update_user_fields(
        &self,
        title_id: &str,
        account_id: &str,
        fields: &UserBookFields,
    ) -> Result<()> {
        let uuid = self.resolve_uuid(title_id, account_id)?;
        let now = Utc::now().to_rfc3339();
        let rows = self.exec_affected(
            r#"
            UPDATE books SET
                tags = COALESCE(?1, tags),
                rating_overall = COALESCE(?2, rating_overall),
                rating_performance = COALESCE(?3, rating_performance),
                rating_story = COALESCE(?4, rating_story),
                is_finished = COALESCE(?5, is_finished),
                updated_at = ?6
            WHERE uuid = ?7
            "#,
            vec![
                fields.tags.clone().into(),
                fields.rating_overall.into(),
                fields.rating_performance.into(),
                fields.rating_story.into(),
                fields.is_finished.map(i64::from).into(),
                now.into(),
                uuid.into(),
            ],
        )?;
        if rows == 0 {
            return Err(LibraryError::NotFound(title_id.into()));
        }
        Ok(())
    }

    pub fn set_pdf_status(
        &self,
        title_id: &str,
        account_id: &str,
        status: AcquireStatus,
        pdf_storage_key: Option<&str>,
    ) -> Result<()> {
        let uuid = self.resolve_uuid(title_id, account_id)?;
        let now = Utc::now().to_rfc3339();
        let rows = self.exec_affected(
            r#"
            UPDATE books SET pdf_status = ?1, pdf_storage_key = ?2, updated_at = ?3
            WHERE uuid = ?4
            "#,
            vec![
                status.as_str().into(),
                pdf_storage_key.into(),
                now.into(),
                uuid.into(),
            ],
        )?;
        if rows == 0 {
            return Err(LibraryError::NotFound(title_id.into()));
        }
        Ok(())
    }

    pub fn is_ignored(&self, title_id: &str, account_id: &str) -> Result<bool> {
        let (source, product_id) = match self.get_book(title_id, account_id)? {
            Some(b) => (b.source, b.product_id),
            None => (String::from("audible"), title_id.to_string()),
        };
        self.exists(
            "SELECT 1 FROM ignored_titles WHERE source = ?1 AND account_id = ?2 AND product_id = ?3",
            vec![source.into(), account_id.into(), product_id.into()],
        )
    }

    pub fn set_ignored(
        &self,
        title_id: &str,
        account_id: &str,
        ignored: bool,
        reason: Option<&str>,
    ) -> Result<()> {
        let (source, product_id) = match self.get_book(title_id, account_id)? {
            Some(b) => (b.source, b.product_id),
            None => (String::from("audible"), title_id.to_string()),
        };
        let now = Utc::now().to_rfc3339();
        if ignored {
            self.exec(
                r#"
                INSERT INTO ignored_titles (source, account_id, product_id, reason, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(source, account_id, product_id) DO UPDATE SET reason = excluded.reason
                "#,
                vec![
                    source.into(),
                    account_id.into(),
                    product_id.into(),
                    reason.into(),
                    now.into(),
                ],
            )?;
        } else {
            self.exec(
                "DELETE FROM ignored_titles WHERE source = ?1 AND account_id = ?2 AND product_id = ?3",
                vec![source.into(), account_id.into(), product_id.into()],
            )?;
        }
        Ok(())
    }

    pub fn set_acquire_status(
        &self,
        title_id: &str,
        account_id: &str,
        status: AcquireStatus,
        storage_key: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<()> {
        let uuid = self.resolve_uuid(title_id, account_id)?;
        let now = Utc::now().to_rfc3339();
        let rows = self.exec_affected(
            r#"
            UPDATE books SET
                acquire_status = ?1,
                storage_key = ?2,
                error_message = ?3,
                updated_at = ?4
            WHERE uuid = ?5
            "#,
            vec![
                status.as_str().into(),
                storage_key.into(),
                error_message.into(),
                now.into(),
                uuid.into(),
            ],
        )?;
        if rows == 0 {
            return Err(LibraryError::NotFound(title_id.into()));
        }
        Ok(())
    }

    /// Bulk-update acquire status (classic `set-status --force`).
    ///
    /// When `asins` is non-empty, matches against uuid, product_id, isbn, or asin.
    pub fn bulk_set_acquire_status(
        &self,
        account: Option<&str>,
        asins: &[String],
        status: AcquireStatus,
    ) -> Result<u32> {
        let books = self.list_books(account)?;
        let mut updated = 0u32;
        for book in books {
            if !asins.is_empty()
                && !asins.iter().any(|a| {
                    a.eq_ignore_ascii_case(&book.uuid)
                        || a.eq_ignore_ascii_case(&book.product_id)
                        || book
                            .isbn
                            .as_ref()
                            .is_some_and(|isbn| a.eq_ignore_ascii_case(isbn))
                        || book
                            .asin
                            .as_ref()
                            .is_some_and(|asin| a.eq_ignore_ascii_case(asin))
                })
            {
                continue;
            }
            self.set_acquire_status(
                &book.uuid,
                &book.account_id,
                status,
                book.storage_key.as_deref(),
                None,
            )?;
            updated += 1;
        }
        Ok(updated)
    }

    /// List saved quick-filter expressions.
    pub fn list_saved_filters(&self) -> Result<Vec<SavedFilterRecord>> {
        self.query_all_rows(
            "SELECT id, name, query, created_at, updated_at FROM saved_filters ORDER BY name",
            vec![],
        )?
        .iter()
        .map(map_saved_filter_row)
        .collect()
    }

    pub fn upsert_saved_filter(&self, name: &str, query: &str) -> Result<SavedFilterRecord> {
        let now = Utc::now().to_rfc3339();
        self.exec(
            r#"
            INSERT INTO saved_filters (name, query, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(name) DO UPDATE SET
                query = excluded.query,
                updated_at = excluded.updated_at
            "#,
            vec![name.into(), query.into(), now.clone().into(), now.into()],
        )?;
        self.get_saved_filter(name)?
            .ok_or_else(|| LibraryError::NotFound(name.into()))
    }

    pub fn get_saved_filter(&self, name: &str) -> Result<Option<SavedFilterRecord>> {
        self.query_one_row(
            "SELECT id, name, query, created_at, updated_at FROM saved_filters WHERE name = ?1",
            vec![name.into()],
        )?
        .map(|r| map_saved_filter_row(&r))
        .transpose()
    }

    pub fn delete_saved_filter(&self, name: &str) -> Result<()> {
        let rows = self.exec_affected(
            "DELETE FROM saved_filters WHERE name = ?1",
            vec![name.into()],
        )?;
        if rows == 0 {
            return Err(LibraryError::NotFound(name.into()));
        }
        Ok(())
    }

    pub fn count_by_status(&self, status: AcquireStatus) -> Result<i64> {
        self.query_one_row(
            "SELECT COUNT(*) AS n FROM books WHERE acquire_status = ?1",
            vec![status.as_str().into()],
        )?
        .ok_or_else(|| LibraryError::Other(anyhow::anyhow!("count query returned no row")))?
        .i64_req("n")
    }

    /// Persist enrichment fields without touching scan / ownership columns.
    pub fn update_catalog_enrichment(
        &self,
        book_uuid: &str,
        fields: &CatalogEnrichmentFields,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let rows = self.exec_affected(
            r#"
            UPDATE books SET
                description = COALESCE(?1, description),
                language = COALESCE(?2, language),
                cover_url = COALESCE(?3, cover_url),
                subjects = COALESCE(?4, subjects),
                categories = COALESCE(?5, categories),
                enrich_source = COALESCE(?6, enrich_source),
                enrich_confidence = COALESCE(?7, enrich_confidence),
                enrich_updated_at = COALESCE(?8, enrich_updated_at),
                updated_at = ?9
            WHERE uuid = ?10
            "#,
            vec![
                fields.description.clone().into(),
                fields.language.clone().into(),
                fields.cover_url.clone().into(),
                fields.subjects.clone().into(),
                fields.categories.clone().into(),
                fields.enrich_source.clone().into(),
                fields.enrich_confidence.into(),
                fields.enrich_updated_at.map(|d| d.to_rfc3339()).into(),
                now.into(),
                book_uuid.into(),
            ],
        )?;
        if rows == 0 {
            return Err(LibraryError::NotFound(book_uuid.into()));
        }
        Ok(())
    }

    /// Upsert a canonical work row.
    pub fn upsert_work(&self, work: &NewWork) -> Result<WorkRecord> {
        let now = Utc::now().to_rfc3339();
        let id = work
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        self.exec(
            r#"
            INSERT INTO works (
                id, canonical_asin, canonical_isbn, title, authors, narrators,
                description, subjects, categories, language, series, series_index,
                cover_url, openlibrary_id, created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
            )
            ON CONFLICT(id) DO UPDATE SET
                canonical_asin = COALESCE(excluded.canonical_asin, works.canonical_asin),
                canonical_isbn = COALESCE(excluded.canonical_isbn, works.canonical_isbn),
                title = excluded.title,
                authors = COALESCE(excluded.authors, works.authors),
                narrators = COALESCE(excluded.narrators, works.narrators),
                description = COALESCE(excluded.description, works.description),
                subjects = COALESCE(excluded.subjects, works.subjects),
                categories = COALESCE(excluded.categories, works.categories),
                language = COALESCE(excluded.language, works.language),
                series = COALESCE(excluded.series, works.series),
                series_index = COALESCE(excluded.series_index, works.series_index),
                cover_url = COALESCE(excluded.cover_url, works.cover_url),
                openlibrary_id = COALESCE(excluded.openlibrary_id, works.openlibrary_id),
                updated_at = excluded.updated_at
            "#,
            vec![
                id.clone().into(),
                work.canonical_asin.clone().into(),
                work.canonical_isbn.clone().into(),
                work.title.clone().into(),
                work.authors.clone().into(),
                work.narrators.clone().into(),
                work.description.clone().into(),
                work.subjects.clone().into(),
                work.categories.clone().into(),
                work.language.clone().into(),
                work.series.clone().into(),
                work.series_index.clone().into(),
                work.cover_url.clone().into(),
                work.openlibrary_id.clone().into(),
                now.clone().into(),
                now.into(),
            ],
        )?;
        self.get_work(&id)?
            .ok_or_else(|| LibraryError::NotFound(id))
    }

    pub fn get_work(&self, id: &str) -> Result<Option<WorkRecord>> {
        self.query_one_row(
            r#"
            SELECT id, canonical_asin, canonical_isbn, title, authors, narrators,
                   description, subjects, categories, language, series, series_index,
                   cover_url, openlibrary_id, created_at, updated_at
            FROM works WHERE id = ?1
            "#,
            vec![id.into()],
        )?
        .map(|r| map_work_row(&r))
        .transpose()
    }

    pub fn find_work_by_asin(&self, asin: &str) -> Result<Option<WorkRecord>> {
        self.query_one_row(
            r#"
            SELECT id, canonical_asin, canonical_isbn, title, authors, narrators,
                   description, subjects, categories, language, series, series_index,
                   cover_url, openlibrary_id, created_at, updated_at
            FROM works WHERE canonical_asin = ?1 LIMIT 1
            "#,
            vec![asin.into()],
        )?
        .map(|r| map_work_row(&r))
        .transpose()
    }

    pub fn find_work_by_isbn(&self, isbn: &str) -> Result<Option<WorkRecord>> {
        self.query_one_row(
            r#"
            SELECT id, canonical_asin, canonical_isbn, title, authors, narrators,
                   description, subjects, categories, language, series, series_index,
                   cover_url, openlibrary_id, created_at, updated_at
            FROM works WHERE canonical_isbn = ?1 LIMIT 1
            "#,
            vec![isbn.into()],
        )?
        .map(|r| map_work_row(&r))
        .transpose()
    }

    pub fn list_works(&self) -> Result<Vec<WorkRecord>> {
        self.query_all_rows(
            r#"
            SELECT id, canonical_asin, canonical_isbn, title, authors, narrators,
                   description, subjects, categories, language, series, series_index,
                   cover_url, openlibrary_id, created_at, updated_at
            FROM works ORDER BY title
            "#,
            vec![],
        )?
        .iter()
        .map(map_work_row)
        .collect()
    }

    pub fn link_book_to_work(&self, work_id: &str, book_uuid: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.exec(
            r#"
            INSERT INTO work_editions (work_id, book_uuid, created_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(book_uuid) DO UPDATE SET work_id = excluded.work_id
            "#,
            vec![work_id.into(), book_uuid.into(), now.into()],
        )?;
        Ok(())
    }

    pub fn work_id_for_book(&self, book_uuid: &str) -> Result<Option<String>> {
        self.query_one_row(
            "SELECT work_id FROM work_editions WHERE book_uuid = ?1",
            vec![book_uuid.into()],
        )?
        .map(|r| r.string_req("work_id"))
        .transpose()
    }

    pub fn book_uuids_for_work(&self, work_id: &str) -> Result<Vec<String>> {
        self.query_all_rows(
            "SELECT book_uuid FROM work_editions WHERE work_id = ?1",
            vec![work_id.into()],
        )?
        .iter()
        .map(|r| r.string_req("book_uuid"))
        .collect()
    }

    pub fn upsert_listening_progress(
        &self,
        row: &NewListeningProgress,
    ) -> Result<ListeningProgressRecord> {
        let now = Utc::now().to_rfc3339();
        self.exec(
            r#"
            INSERT INTO listening_progress (
                identity_id, provider, external_user_id, book_uuid, work_id,
                external_item_id, title, authors, asin, isbn, progress,
                current_time_seconds, duration_seconds, is_finished,
                last_listened_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
            )
            ON CONFLICT(provider, external_user_id, external_item_id) DO UPDATE SET
                identity_id = COALESCE(excluded.identity_id, listening_progress.identity_id),
                book_uuid = COALESCE(excluded.book_uuid, listening_progress.book_uuid),
                work_id = COALESCE(excluded.work_id, listening_progress.work_id),
                title = COALESCE(excluded.title, listening_progress.title),
                authors = COALESCE(excluded.authors, listening_progress.authors),
                asin = COALESCE(excluded.asin, listening_progress.asin),
                isbn = COALESCE(excluded.isbn, listening_progress.isbn),
                progress = excluded.progress,
                current_time_seconds = excluded.current_time_seconds,
                duration_seconds = excluded.duration_seconds,
                is_finished = excluded.is_finished,
                last_listened_at = COALESCE(
                    excluded.last_listened_at, listening_progress.last_listened_at
                ),
                updated_at = excluded.updated_at
            "#,
            vec![
                row.identity_id.into(),
                row.provider.clone().into(),
                row.external_user_id.clone().into(),
                row.book_uuid.clone().into(),
                row.work_id.clone().into(),
                row.external_item_id.clone().into(),
                row.title.clone().into(),
                row.authors.clone().into(),
                row.asin.clone().into(),
                row.isbn.clone().into(),
                row.progress.into(),
                row.current_time_seconds.into(),
                row.duration_seconds.into(),
                i64::from(row.is_finished).into(),
                row.last_listened_at.map(|d| d.to_rfc3339()).into(),
                now.into(),
            ],
        )?;
        self.get_listening_progress(&row.provider, &row.external_user_id, &row.external_item_id)?
            .ok_or_else(|| LibraryError::NotFound(row.external_item_id.clone()))
    }

    pub fn get_listening_progress(
        &self,
        provider: &str,
        external_user_id: &str,
        external_item_id: &str,
    ) -> Result<Option<ListeningProgressRecord>> {
        self.query_one_row(
            r#"
            SELECT id, identity_id, provider, external_user_id, book_uuid, work_id,
                   external_item_id, title, authors, asin, isbn, progress,
                   current_time_seconds, duration_seconds, is_finished,
                   last_listened_at, updated_at
            FROM listening_progress
            WHERE provider = ?1 AND external_user_id = ?2 AND external_item_id = ?3
            "#,
            vec![
                provider.into(),
                external_user_id.into(),
                external_item_id.into(),
            ],
        )?
        .map(|r| map_listening_row(&r))
        .transpose()
    }

    pub fn list_listening_progress(
        &self,
        external_user_id: Option<&str>,
    ) -> Result<Vec<ListeningProgressRecord>> {
        let rows = if let Some(uid) = external_user_id {
            self.query_all_rows(
                r#"
                SELECT id, identity_id, provider, external_user_id, book_uuid, work_id,
                       external_item_id, title, authors, asin, isbn, progress,
                       current_time_seconds, duration_seconds, is_finished,
                       last_listened_at, updated_at
                FROM listening_progress
                WHERE external_user_id = ?1
                ORDER BY COALESCE(last_listened_at, updated_at) DESC
                "#,
                vec![uid.into()],
            )?
        } else {
            self.query_all_rows(
                r#"
                SELECT id, identity_id, provider, external_user_id, book_uuid, work_id,
                       external_item_id, title, authors, asin, isbn, progress,
                       current_time_seconds, duration_seconds, is_finished,
                       last_listened_at, updated_at
                FROM listening_progress
                ORDER BY COALESCE(last_listened_at, updated_at) DESC
                "#,
                vec![],
            )?
        };
        rows.iter().map(map_listening_row).collect()
    }

    pub fn create_title_request(&self, req: &NewTitleRequest) -> Result<TitleRequestRecord> {
        let now = Utc::now().to_rfc3339();
        let uuid = req
            .uuid
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let work_key = if req.work_key.trim().is_empty() {
            fallback_work_key(
                &req.title,
                req.authors.as_deref(),
                req.asin.as_deref(),
                req.isbn.as_deref(),
            )
        } else {
            req.work_key.trim().to_string()
        };

        // Idempotent wishlist: same wisher + exact work_key, or same bibliographic
        // identity under a different key (e.g. soft:… vs asin:… / isbn:…).
        if let Some(existing) = self.find_open_wishlist(req.identity_id, &work_key)? {
            return Ok(existing);
        }
        if let Some(existing) = self.find_open_wishlist_matching(
            req.identity_id,
            &work_key,
            &req.title,
            req.authors.as_deref(),
            req.asin.as_deref(),
            req.isbn.as_deref(),
        )? {
            return Ok(existing);
        }

        self.exec(
            r#"
            INSERT INTO title_requests (
                uuid, identity_id, title, authors, asin, isbn, notes, status,
                work_key, work_id, resolved_book_uuid, created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
            )
            "#,
            vec![
                uuid.clone().into(),
                req.identity_id.into(),
                req.title.clone().into(),
                req.authors.clone().into(),
                req.asin.clone().into(),
                req.isbn.clone().into(),
                req.notes.clone().into(),
                req.status.as_str().into(),
                work_key.into(),
                req.work_id.clone().into(),
                req.resolved_book_uuid.clone().into(),
                now.clone().into(),
                now.into(),
            ],
        )?;
        self.get_title_request_by_uuid(&uuid)?
            .ok_or_else(|| LibraryError::NotFound(uuid))
    }

    /// Open wishlist row for this identity + work key, if any.
    pub fn find_open_wishlist(
        &self,
        identity_id: Option<i64>,
        work_key: &str,
    ) -> Result<Option<TitleRequestRecord>> {
        let work_key = work_key.trim();
        if work_key.is_empty() {
            return Ok(None);
        }
        let row = if let Some(id) = identity_id {
            self.query_one_row(
                r#"
                SELECT id, uuid, identity_id, title, authors, asin, isbn, notes, status,
                       work_key, work_id, resolved_book_uuid,
                       created_at, updated_at
                FROM title_requests
                WHERE identity_id = ?1 AND work_key = ?2 AND status = 'open'
                ORDER BY created_at DESC LIMIT 1
                "#,
                vec![id.into(), work_key.into()],
            )?
        } else {
            self.query_one_row(
                r#"
                SELECT id, uuid, identity_id, title, authors, asin, isbn, notes, status,
                       work_key, work_id, resolved_book_uuid,
                       created_at, updated_at
                FROM title_requests
                WHERE identity_id IS NULL AND work_key = ?1 AND status = 'open'
                ORDER BY created_at DESC LIMIT 1
                "#,
                vec![work_key.into()],
            )?
        };
        row.map(|r| map_request_row(&r)).transpose()
    }

    /// Open wishlist row that matches bibliographic identity even when `work_key` differs.
    pub fn find_open_wishlist_matching(
        &self,
        identity_id: Option<i64>,
        work_key: &str,
        title: &str,
        authors: Option<&str>,
        asin: Option<&str>,
        isbn: Option<&str>,
    ) -> Result<Option<TitleRequestRecord>> {
        let needle = WishlistIdentity {
            work_key,
            title,
            authors,
            asin,
            isbn,
        };
        let open = self.list_wishlist(identity_id)?;
        Ok(open.into_iter().find(|row| {
            needle.matches(WishlistIdentity {
                work_key: &row.work_key,
                title: &row.title,
                authors: row.authors.as_deref(),
                asin: row.asin.as_deref(),
                isbn: row.isbn.as_deref(),
            })
        }))
    }

    /// Personal open wishlist for a portal identity, or operator (`identity_id` null).
    pub fn list_wishlist(&self, identity_id: Option<i64>) -> Result<Vec<TitleRequestRecord>> {
        let rows = if let Some(id) = identity_id {
            self.query_all_rows(
                r#"
                SELECT id, uuid, identity_id, title, authors, asin, isbn, notes, status,
                       work_key, work_id, resolved_book_uuid,
                       created_at, updated_at
                FROM title_requests
                WHERE identity_id = ?1 AND status = 'open'
                ORDER BY created_at DESC
                "#,
                vec![id.into()],
            )?
        } else {
            self.query_all_rows(
                r#"
                SELECT id, uuid, identity_id, title, authors, asin, isbn, notes, status,
                       work_key, work_id, resolved_book_uuid,
                       created_at, updated_at
                FROM title_requests
                WHERE identity_id IS NULL AND status = 'open'
                ORDER BY created_at DESC
                "#,
                vec![],
            )?
        };
        rows.iter().map(map_request_row).collect()
    }

    /// Global request queue: open wishes grouped by `work_key`.
    ///
    /// Sorted by wish count as a simple default; Discover re-ranks with local
    /// taste plus a heavy per-wisher boost for the Wishlist sidebar.
    pub fn list_global_request_queue(&self) -> Result<Vec<GlobalQueueEntry>> {
        let open = self.list_title_requests(Some(RequestStatus::Open))?;
        let mut by_key: std::collections::HashMap<String, GlobalQueueEntry> =
            std::collections::HashMap::new();
        for row in open {
            let key = if row.work_key.trim().is_empty() {
                fallback_work_key(
                    &row.title,
                    row.authors.as_deref(),
                    row.asin.as_deref(),
                    row.isbn.as_deref(),
                )
            } else {
                row.work_key.clone()
            };
            match by_key.entry(key.clone()) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(GlobalQueueEntry {
                        work_key: key,
                        title: row.title,
                        authors: row.authors,
                        asin: row.asin,
                        isbn: row.isbn,
                        wish_count: 1,
                        sample_uuids: vec![row.uuid],
                        first_requested_at: row.created_at,
                        last_requested_at: row.created_at,
                    });
                }
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    let entry = e.get_mut();
                    entry.wish_count += 1;
                    if entry.sample_uuids.len() < 8 {
                        entry.sample_uuids.push(row.uuid);
                    }
                    if row.created_at < entry.first_requested_at {
                        entry.first_requested_at = row.created_at;
                    }
                    if row.created_at > entry.last_requested_at {
                        entry.last_requested_at = row.created_at;
                        // Prefer the newest metadata.
                        entry.title = row.title;
                        if row.authors.is_some() {
                            entry.authors = row.authors;
                        }
                        if row.asin.is_some() {
                            entry.asin = row.asin;
                        }
                        if row.isbn.is_some() {
                            entry.isbn = row.isbn;
                        }
                    }
                }
            }
        }
        let mut out: Vec<_> = by_key.into_values().collect();
        out.sort_by(|a, b| {
            b.wish_count
                .cmp(&a.wish_count)
                .then_with(|| b.last_requested_at.cmp(&a.last_requested_at))
        });
        Ok(out)
    }

    pub fn get_title_request_by_uuid(&self, uuid: &str) -> Result<Option<TitleRequestRecord>> {
        self.query_one_row(
            r#"
            SELECT id, uuid, identity_id, title, authors, asin, isbn, notes, status,
                   work_key, work_id, resolved_book_uuid,
                   created_at, updated_at
            FROM title_requests WHERE uuid = ?1
            "#,
            vec![uuid.into()],
        )?
        .map(|r| map_request_row(&r))
        .transpose()
    }

    pub fn list_title_requests(
        &self,
        status: Option<RequestStatus>,
    ) -> Result<Vec<TitleRequestRecord>> {
        let rows = if let Some(status) = status {
            self.query_all_rows(
                r#"
                SELECT id, uuid, identity_id, title, authors, asin, isbn, notes, status,
                       work_key, work_id, resolved_book_uuid,
                       created_at, updated_at
                FROM title_requests WHERE status = ?1
                ORDER BY created_at DESC
                "#,
                vec![status.as_str().into()],
            )?
        } else {
            self.query_all_rows(
                r#"
                SELECT id, uuid, identity_id, title, authors, asin, isbn, notes, status,
                       work_key, work_id, resolved_book_uuid,
                       created_at, updated_at
                FROM title_requests
                ORDER BY created_at DESC
                "#,
                vec![],
            )?
        };
        rows.iter().map(map_request_row).collect()
    }

    pub fn update_title_request_status(
        &self,
        uuid: &str,
        status: RequestStatus,
        resolved_book_uuid: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let rows = self.exec_affected(
            r#"
            UPDATE title_requests SET
                status = ?1,
                resolved_book_uuid = COALESCE(?2, resolved_book_uuid),
                updated_at = ?3
            WHERE uuid = ?4
            "#,
            vec![
                status.as_str().into(),
                resolved_book_uuid.into(),
                now.into(),
                uuid.into(),
            ],
        )?;
        if rows == 0 {
            return Err(LibraryError::NotFound(uuid.into()));
        }
        Ok(())
    }

    pub fn upsert_embedding(
        &self,
        target_kind: &str,
        target_id: &str,
        model: &str,
        dims: i64,
        vector: &[u8],
        text_hash: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.exec(
            r#"
            INSERT INTO embeddings (
                target_kind, target_id, model, dims, vector, text_hash, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(target_kind, target_id, model) DO UPDATE SET
                dims = excluded.dims,
                vector = excluded.vector,
                text_hash = excluded.text_hash,
                updated_at = excluded.updated_at
            "#,
            vec![
                target_kind.into(),
                target_id.into(),
                model.into(),
                dims.into(),
                vector.into(),
                text_hash.into(),
                now.clone().into(),
                now.into(),
            ],
        )?;
        Ok(())
    }

    pub fn get_embedding_vector(
        &self,
        target_kind: &str,
        target_id: &str,
        model: &str,
    ) -> Result<Option<(String, Vec<u8>)>> {
        self.query_one_row(
            r#"
            SELECT text_hash, vector FROM embeddings
            WHERE target_kind = ?1 AND target_id = ?2 AND model = ?3
            "#,
            vec![target_kind.into(), target_id.into(), model.into()],
        )?
        .map(|r| Ok((r.string_req("text_hash")?, r.bytes_req("vector")?)))
        .transpose()
    }

    pub fn list_embeddings(
        &self,
        target_kind: &str,
        model: &str,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        self.query_all_rows(
            r#"
            SELECT target_id, vector FROM embeddings
            WHERE target_kind = ?1 AND model = ?2
            "#,
            vec![target_kind.into(), model.into()],
        )?
        .iter()
        .map(|r| Ok((r.string_req("target_id")?, r.bytes_req("vector")?)))
        .collect()
    }

    pub fn embedding_text_hash(
        &self,
        target_kind: &str,
        target_id: &str,
        model: &str,
    ) -> Result<Option<String>> {
        self.query_one_row(
            r#"
            SELECT text_hash FROM embeddings
            WHERE target_kind = ?1 AND target_id = ?2 AND model = ?3
            "#,
            vec![target_kind.into(), target_id.into(), model.into()],
        )?
        .map(|r| r.string_req("text_hash"))
        .transpose()
    }

    /// Load per-user GUI / Discover preferences by subject key.
    pub fn get_user_preferences(&self, subject_key: &str) -> Result<Option<UserPreferences>> {
        self.query_one_row(
            r#"
            SELECT id, subject_key, identity_id, default_view, disabled_shelves_json, updated_at
            FROM user_preferences
            WHERE subject_key = ?1
            "#,
            vec![subject_key.into()],
        )?
        .map(|r| map_user_preferences_row(&r))
        .transpose()
    }

    /// Preferences for `subject_key`, or in-memory defaults when no row exists.
    pub fn get_user_preferences_or_default(
        &self,
        subject_key: &str,
        identity_id: Option<i64>,
    ) -> Result<UserPreferences> {
        Ok(self
            .get_user_preferences(subject_key)?
            .unwrap_or_else(|| UserPreferences::defaults_for(subject_key, identity_id)))
    }

    /// Insert or replace preferences for a subject (operator or portal identity).
    pub fn upsert_user_preferences(
        &self,
        subject_key: &str,
        identity_id: Option<i64>,
        default_view: &str,
        disabled_shelves: &[String],
    ) -> Result<UserPreferences> {
        let now = Utc::now().to_rfc3339();
        let shelves_json =
            serde_json::to_string(disabled_shelves).unwrap_or_else(|_| String::from("[]"));
        self.exec(
            r#"
            INSERT INTO user_preferences (
                subject_key, identity_id, default_view, disabled_shelves_json, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(subject_key) DO UPDATE SET
                identity_id = COALESCE(excluded.identity_id, user_preferences.identity_id),
                default_view = excluded.default_view,
                disabled_shelves_json = excluded.disabled_shelves_json,
                updated_at = excluded.updated_at
            "#,
            vec![
                subject_key.into(),
                identity_id.into(),
                default_view.into(),
                shelves_json.into(),
                now.into(),
            ],
        )?;
        self.get_user_preferences(subject_key)?
            .ok_or_else(|| LibraryError::NotFound(subject_key.into()))
    }
}

/// Prefer an Audible ownership row with the richest metadata for enrichment.
#[must_use]
pub fn prefer_enrichment_source(books: &[BookRecord]) -> Option<&BookRecord> {
    books.iter().max_by_key(|b| {
        let mut score = 0u32;
        // Prefer rows that already carry an Audible ASIN (ownership or enrichment).
        if b.asin.is_some() {
            score += 100;
        }
        if b.isbn.is_some() {
            score += 10;
        }
        if b.authors.is_some() {
            score += 2;
        }
        if b.narrators.is_some() {
            score += 2;
        }
        if b.series.is_some() {
            score += 2;
        }
        if b.publisher.is_some() {
            score += 1;
        }
        if b.subtitle.is_some() {
            score += 1;
        }
        if b.length_minutes.is_some() {
            score += 1;
        }
        if b.categories.is_some() {
            score += 1;
        }
        if !b.title.is_empty() {
            score += 1;
        }
        score
    })
}

/// Input for inserting / updating a book from sync.
#[derive(Debug, Clone)]
pub struct NewBook {
    /// Public id; generated on insert when `None`.
    pub uuid: Option<String>,
    pub product_id: String,
    pub source: String,
    pub account_id: String,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    pub marketplace: String,
    pub title: String,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub series_asin: Option<String>,
    pub purchased_at: Option<chrono::DateTime<Utc>>,
    pub publisher: Option<String>,
    pub length_minutes: Option<i64>,
    pub is_abridged: bool,
    pub content_kind: String,
    pub categories: Option<String>,
    pub subtitle: Option<String>,
    pub published_at: Option<chrono::DateTime<Utc>>,
}

impl NewBook {
    /// Minimal Audible book row with scan metadata defaults.
    ///
    /// Sets `product_id` from the first argument, `asin = Some(product_id)`,
    /// `isbn = None`, `uuid = None` (auto), and `source = "audible"`.
    #[must_use]
    pub fn minimal(
        product_id: impl Into<String>,
        account_id: impl Into<String>,
        marketplace: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        let product_id = product_id.into();
        Self {
            uuid: None,
            asin: Some(product_id.clone()),
            isbn: None,
            product_id,
            source: String::from("audible"),
            account_id: account_id.into(),
            marketplace: marketplace.into(),
            title: title.into(),
            authors: None,
            narrators: None,
            series: None,
            series_index: None,
            series_asin: None,
            purchased_at: None,
            publisher: None,
            length_minutes: None,
            is_abridged: false,
            content_kind: String::from("book"),
            categories: None,
            subtitle: None,
            published_at: None,
        }
    }
}

/// A single result row from the SeaORM proxy, keyed by column name.
///
/// Wraps the proxy's `BTreeMap<String, Value>` and normalises the sqlite/D1
/// proxy quirks: integers come back as `BigInt`, reals as `Double`, NULLs as
/// `String(None)`. Access columns **by name** — the proxy does not preserve
/// SELECT column order for positional access.
struct Row {
    values: BTreeMap<String, Value>,
}

impl Row {
    fn from_query(qr: &QueryResult) -> Self {
        Self {
            values: sea_orm::from_query_result_to_proxy_row(qr).values,
        }
    }

    fn raw(&self, col: &str) -> Result<&Value> {
        self.values
            .get(col)
            .ok_or_else(|| LibraryError::Other(anyhow::anyhow!("missing column `{col}`")))
    }

    fn i64_opt(&self, col: &str) -> Result<Option<i64>> {
        match self.raw(col)? {
            Value::BigInt(Some(n)) => Ok(Some(*n)),
            Value::Int(Some(n)) => Ok(Some(i64::from(*n))),
            Value::SmallInt(Some(n)) => Ok(Some(i64::from(*n))),
            Value::TinyInt(Some(n)) => Ok(Some(i64::from(*n))),
            Value::Bool(Some(b)) => Ok(Some(i64::from(*b))),
            Value::Double(Some(n)) => Ok(Some(*n as i64)),
            Value::Float(Some(n)) => Ok(Some(*n as i64)),
            Value::String(Some(s)) => s.parse::<i64>().map(Some).map_err(|e| {
                LibraryError::Other(anyhow::anyhow!("column `{col}` not an integer: {e}"))
            }),
            v if value_is_null(v) => Ok(None),
            other => Err(LibraryError::Other(anyhow::anyhow!(
                "column `{col}` unexpected type: {other:?}"
            ))),
        }
    }

    fn i64_req(&self, col: &str) -> Result<i64> {
        self.i64_opt(col)?
            .ok_or_else(|| LibraryError::Other(anyhow::anyhow!("column `{col}` is null")))
    }

    fn bool_int(&self, col: &str) -> Result<bool> {
        Ok(self.i64_opt(col)?.unwrap_or(0) != 0)
    }

    fn string_opt(&self, col: &str) -> Result<Option<String>> {
        match self.raw(col)? {
            Value::String(Some(s)) => Ok(Some(s.clone())),
            Value::BigInt(Some(n)) => Ok(Some(n.to_string())),
            Value::Int(Some(n)) => Ok(Some(n.to_string())),
            Value::Double(Some(n)) => Ok(Some(n.to_string())),
            Value::Float(Some(n)) => Ok(Some(n.to_string())),
            Value::Bytes(Some(b)) => Ok(Some(String::from_utf8_lossy(b).into_owned())),
            v if value_is_null(v) => Ok(None),
            other => Err(LibraryError::Other(anyhow::anyhow!(
                "column `{col}` unexpected type: {other:?}"
            ))),
        }
    }

    fn string_req(&self, col: &str) -> Result<String> {
        self.string_opt(col)?
            .ok_or_else(|| LibraryError::Other(anyhow::anyhow!("column `{col}` is null")))
    }

    /// `TEXT` column with a default when NULL or absent.
    fn string_or(&self, col: &str, default: &str) -> String {
        self.string_opt(col)
            .ok()
            .flatten()
            .unwrap_or_else(|| default.to_string())
    }

    fn f64_opt(&self, col: &str) -> Result<Option<f64>> {
        match self.raw(col)? {
            Value::Double(Some(n)) => Ok(Some(*n)),
            Value::Float(Some(n)) => Ok(Some(f64::from(*n))),
            Value::BigInt(Some(n)) => Ok(Some(*n as f64)),
            Value::Int(Some(n)) => Ok(Some(f64::from(*n))),
            Value::String(Some(s)) => s.parse::<f64>().map(Some).map_err(|e| {
                LibraryError::Other(anyhow::anyhow!("column `{col}` not a number: {e}"))
            }),
            v if value_is_null(v) => Ok(None),
            other => Err(LibraryError::Other(anyhow::anyhow!(
                "column `{col}` unexpected type: {other:?}"
            ))),
        }
    }

    fn f32_opt(&self, col: &str) -> Result<Option<f32>> {
        Ok(self.f64_opt(col)?.map(|n| n as f32))
    }

    fn bytes_opt(&self, col: &str) -> Result<Option<Vec<u8>>> {
        match self.raw(col)? {
            Value::Bytes(Some(b)) => Ok(Some(b.clone())),
            Value::String(Some(s)) => Ok(Some(s.clone().into_bytes())),
            v if value_is_null(v) => Ok(None),
            other => Err(LibraryError::Other(anyhow::anyhow!(
                "column `{col}` unexpected type: {other:?}"
            ))),
        }
    }

    fn bytes_req(&self, col: &str) -> Result<Vec<u8>> {
        self.bytes_opt(col)?
            .ok_or_else(|| LibraryError::Other(anyhow::anyhow!("column `{col}` is null")))
    }
}

/// Whether a proxy [`Value`] represents SQL NULL (any `*(None)` variant).
fn value_is_null(v: &Value) -> bool {
    matches!(
        v,
        Value::Bool(None)
            | Value::TinyInt(None)
            | Value::SmallInt(None)
            | Value::Int(None)
            | Value::BigInt(None)
            | Value::Float(None)
            | Value::Double(None)
            | Value::String(None)
            | Value::Bytes(None)
    )
}

fn map_account_row(r: &Row) -> Result<AccountRecord> {
    Ok(AccountRecord {
        id: r.i64_req("id")?,
        account_id: r.string_req("account_id")?,
        source: r.string_or("source", "audible"),
        marketplace: r.string_req("marketplace")?,
        label: r.string_opt("label")?,
        scan_enabled: r.bool_int("scan_enabled")?,
        connection_status: r.string_or("connection_status", "active"),
        created_at: parse_dt(&r.string_req("created_at")?),
        updated_at: parse_dt(&r.string_req("updated_at")?),
    })
}

fn map_portal_identity_row(r: &Row) -> Result<crate::models::PortalIdentity> {
    Ok(crate::models::PortalIdentity {
        id: r.i64_req("id")?,
        provider: r.string_req("provider")?,
        external_user_id: r.string_req("external_user_id")?,
        label: r.string_opt("label")?,
        created_at: parse_dt(&r.string_req("created_at")?),
    })
}

fn map_claim_ticket_row(r: &Row) -> Result<crate::models::ClaimTicketRecord> {
    Ok(crate::models::ClaimTicketRecord {
        id: r.i64_req("id")?,
        token_hash: r.string_req("token_hash")?,
        identity_id: r.i64_opt("identity_id")?,
        expires_at: parse_dt(&r.string_req("expires_at")?),
        redeemed_at: r.string_opt("redeemed_at")?.as_deref().map(parse_dt),
        created_by: r.string_req("created_by")?,
        created_at: parse_dt(&r.string_req("created_at")?),
    })
}

fn map_account_link_row(r: &Row) -> Result<crate::models::AccountLinkRecord> {
    Ok(crate::models::AccountLinkRecord {
        id: r.i64_req("id")?,
        identity_id: r.i64_req("identity_id")?,
        account_id: r.string_req("account_id")?,
        source: r.string_req("source")?,
        created_at: parse_dt(&r.string_req("created_at")?),
    })
}

fn map_user_preferences_row(r: &Row) -> Result<UserPreferences> {
    let shelves_json = r.string_or("disabled_shelves_json", "[]");
    let disabled_shelves: Vec<String> = serde_json::from_str(&shelves_json).unwrap_or_default();
    Ok(UserPreferences {
        id: r.i64_req("id")?,
        subject_key: r.string_req("subject_key")?,
        identity_id: r.i64_opt("identity_id")?,
        default_view: r.string_req("default_view")?,
        disabled_shelves,
        updated_at: parse_dt(&r.string_req("updated_at")?),
    })
}

/// Saved Lucene-style quick filter.
#[derive(Debug, Clone)]
pub struct SavedFilterRecord {
    pub id: i64,
    pub name: String,
    pub query: String,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

/// Partial update for user-defined book fields.
#[derive(Debug, Clone, Default)]
pub struct UserBookFields {
    pub tags: Option<String>,
    pub rating_overall: Option<f32>,
    pub rating_performance: Option<f32>,
    pub rating_story: Option<f32>,
    pub is_finished: Option<bool>,
}

/// Durable catalog enrichment fields (blurbs, subjects, provenance).
#[derive(Debug, Clone, Default)]
pub struct CatalogEnrichmentFields {
    pub description: Option<String>,
    pub language: Option<String>,
    pub cover_url: Option<String>,
    pub subjects: Option<String>,
    pub categories: Option<String>,
    pub enrich_source: Option<String>,
    pub enrich_confidence: Option<f64>,
    pub enrich_updated_at: Option<chrono::DateTime<Utc>>,
}

/// Input for upserting a canonical work.
#[derive(Debug, Clone, Default)]
pub struct NewWork {
    pub id: Option<String>,
    pub canonical_asin: Option<String>,
    pub canonical_isbn: Option<String>,
    pub title: String,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub description: Option<String>,
    pub subjects: Option<String>,
    pub categories: Option<String>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub cover_url: Option<String>,
    pub openlibrary_id: Option<String>,
}

/// Input for upserting listening progress.
#[derive(Debug, Clone)]
pub struct NewListeningProgress {
    pub identity_id: Option<i64>,
    pub provider: String,
    pub external_user_id: String,
    pub book_uuid: Option<String>,
    pub work_id: Option<String>,
    pub external_item_id: String,
    pub title: Option<String>,
    pub authors: Option<String>,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    pub progress: Option<f64>,
    pub current_time_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub is_finished: bool,
    pub last_listened_at: Option<chrono::DateTime<Utc>>,
}

/// Input for creating a title request / wishlist row.
#[derive(Debug, Clone)]
pub struct NewTitleRequest {
    pub uuid: Option<String>,
    pub identity_id: Option<i64>,
    pub title: String,
    pub authors: Option<String>,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    pub notes: Option<String>,
    pub status: RequestStatus,
    /// Stable bibliographic key; empty triggers [`fallback_work_key`].
    pub work_key: String,
    pub work_id: Option<String>,
    pub resolved_book_uuid: Option<String>,
}

/// Bibliographic slice used for wishlist dedupe.
#[derive(Debug, Clone, Copy)]
pub struct WishlistIdentity<'a> {
    pub work_key: &'a str,
    pub title: &'a str,
    pub authors: Option<&'a str>,
    pub asin: Option<&'a str>,
    pub isbn: Option<&'a str>,
}

impl WishlistIdentity<'_> {
    /// Whether two wishlist rows refer to the same work despite differing keys.
    ///
    /// Matches exact `work_key`, shared ASIN/ISBN, or the soft title+author key
    /// from [`fallback_work_key`]. Cross-key isbn↔asin pairs require a soft title
    /// agreement so unrelated hard keys never collapse.
    #[must_use]
    pub fn matches(self, other: WishlistIdentity<'_>) -> bool {
        let ka = self.work_key.trim();
        let kb = other.work_key.trim();
        if !ka.is_empty() && !kb.is_empty() && ka == kb {
            return true;
        }
        let soft_a = fallback_work_key(self.title, self.authors, None, None);
        let soft_b = fallback_work_key(other.title, other.authors, None, None);
        let soft_ok = soft_a != "soft:|" && soft_a == soft_b;

        if let (Some(aa), Some(ab)) = (
            self.asin.map(str::trim).filter(|s| !s.is_empty()),
            other.asin.map(str::trim).filter(|s| !s.is_empty()),
        ) {
            if aa.eq_ignore_ascii_case(ab) {
                return true;
            }
        }
        let isbn_a = fallback_work_key("", None, None, self.isbn);
        let isbn_b = fallback_work_key("", None, None, other.isbn);
        if isbn_a.starts_with("isbn:") && isbn_a == isbn_b {
            return true;
        }
        // Stored work_key may be asin:/isbn: while the other side only has fields.
        if !ka.is_empty() {
            if ka.starts_with("asin:") {
                if let Some(ab) = other.asin.map(str::trim).filter(|s| !s.is_empty()) {
                    if ka.eq_ignore_ascii_case(&format!("asin:{}", ab.to_ascii_uppercase())) {
                        return true;
                    }
                }
            }
            if ka.starts_with("isbn:") && ka == isbn_b {
                return true;
            }
            if ka.starts_with("soft:") && ka == soft_b {
                return true;
            }
        }
        if !kb.is_empty() {
            if kb.starts_with("asin:") {
                if let Some(aa) = self.asin.map(str::trim).filter(|s| !s.is_empty()) {
                    if kb.eq_ignore_ascii_case(&format!("asin:{}", aa.to_ascii_uppercase())) {
                        return true;
                    }
                }
            }
            if kb.starts_with("isbn:") && kb == isbn_a {
                return true;
            }
            if kb.starts_with("soft:") && kb == soft_a {
                return true;
            }
        }
        let cross = (ka.starts_with("isbn:") && kb.starts_with("asin:"))
            || (ka.starts_with("asin:") && kb.starts_with("isbn:"))
            || (isbn_a.starts_with("isbn:")
                && other.asin.map(str::trim).is_some_and(|s| !s.is_empty()))
            || (isbn_b.starts_with("isbn:")
                && self.asin.map(str::trim).is_some_and(|s| !s.is_empty()));
        if cross {
            return soft_ok;
        }
        soft_ok
    }
}

/// Whether two wishlist rows refer to the same work despite differing keys.
#[must_use]
pub fn wishlist_identities_match(a: WishlistIdentity<'_>, b: WishlistIdentity<'_>) -> bool {
    a.matches(b)
}

/// Local fallback when callers do not supply a discover `work_map_key`.
///
/// Converts ISBN-10 → ISBN-13 when possible so 10/13 variants share a key.
/// Soft keys here are a simple lowercase fallback — Discover re-merges with
/// richer identity matching when ranking the global queue.
#[must_use]
pub fn fallback_work_key(
    title: &str,
    authors: Option<&str>,
    asin: Option<&str>,
    isbn: Option<&str>,
) -> String {
    let mut isbn_digits: String = isbn
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .collect::<String>()
        .to_ascii_uppercase();
    if isbn_digits.len() == 10 {
        let core = &isbn_digits[..9];
        if core.chars().all(|c| c.is_ascii_digit()) {
            let mut body = String::from("978");
            body.push_str(core);
            let mut sum = 0u32;
            for (i, c) in body.chars().enumerate() {
                let d = c.to_digit(10).unwrap_or(0);
                sum += if i % 2 == 0 { d } else { d * 3 };
            }
            let check = (10 - (sum % 10)) % 10;
            body.push(char::from_digit(check, 10).unwrap_or('0'));
            isbn_digits = body;
        }
    }
    if !isbn_digits.is_empty() {
        return format!("isbn:{isbn_digits}");
    }
    if let Some(asin) = asin.map(str::trim).filter(|s| !s.is_empty()) {
        return format!("asin:{}", asin.to_ascii_uppercase());
    }
    let t = title.trim().to_ascii_lowercase();
    let a = authors
        .unwrap_or("")
        .split([',', ';', '&', '/'])
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase();
    if t.is_empty() && a.is_empty() {
        return String::from("title:");
    }
    format!("soft:{t}|{a}")
}

fn map_book_row(r: &Row) -> Result<BookRecord> {
    let status_raw = r.string_or("acquire_status", "not_acquired");
    let pdf_raw = r.string_or("pdf_status", "not_acquired");
    Ok(BookRecord {
        id: r.i64_req("id")?,
        uuid: r.string_req("uuid")?,
        source: r.string_or("source", "audible"),
        account_id: r.string_req("account_id")?,
        product_id: r.string_req("product_id")?,
        asin: r.string_opt("asin")?,
        isbn: r.string_opt("isbn")?,
        marketplace: r.string_req("marketplace")?,
        title: r.string_req("title")?,
        authors: r.string_opt("authors")?,
        narrators: r.string_opt("narrators")?,
        series: r.string_opt("series")?,
        series_index: r.string_opt("series_index")?,
        series_asin: r.string_opt("series_asin")?,
        acquire_status: AcquireStatus::parse(&status_raw).unwrap_or_default(),
        storage_key: r.string_opt("storage_key")?,
        error_message: r.string_opt("error_message")?,
        purchased_at: r.string_opt("purchased_at")?.as_deref().map(parse_dt),
        tags: r.string_opt("tags")?,
        rating_overall: r.f32_opt("rating_overall")?,
        rating_performance: r.f32_opt("rating_performance")?,
        rating_story: r.f32_opt("rating_story")?,
        is_finished: r.bool_int("is_finished")?,
        pdf_status: AcquireStatus::parse(&pdf_raw).unwrap_or_default(),
        pdf_storage_key: r.string_opt("pdf_storage_key")?,
        publisher: r.string_opt("publisher")?,
        length_minutes: r.i64_opt("length_minutes")?,
        is_abridged: r.bool_int("is_abridged")?,
        content_kind: r.string_or("content_kind", "book"),
        categories: r.string_opt("categories")?,
        subtitle: r.string_opt("subtitle")?,
        published_at: r.string_opt("published_at")?.as_deref().map(parse_dt),
        description: r.string_opt("description")?,
        language: r.string_opt("language")?,
        cover_url: r.string_opt("cover_url")?,
        subjects: r.string_opt("subjects")?,
        enrich_source: r.string_opt("enrich_source")?,
        enrich_confidence: r.f64_opt("enrich_confidence")?,
        enrich_updated_at: r.string_opt("enrich_updated_at")?.as_deref().map(parse_dt),
        created_at: parse_dt(&r.string_req("created_at")?),
        updated_at: parse_dt(&r.string_req("updated_at")?),
    })
}

fn parse_dt(value: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn map_saved_filter_row(r: &Row) -> Result<SavedFilterRecord> {
    Ok(SavedFilterRecord {
        id: r.i64_req("id")?,
        name: r.string_req("name")?,
        query: r.string_req("query")?,
        created_at: parse_dt(&r.string_req("created_at")?),
        updated_at: parse_dt(&r.string_req("updated_at")?),
    })
}

fn map_work_row(r: &Row) -> Result<WorkRecord> {
    Ok(WorkRecord {
        id: r.string_req("id")?,
        canonical_asin: r.string_opt("canonical_asin")?,
        canonical_isbn: r.string_opt("canonical_isbn")?,
        title: r.string_req("title")?,
        authors: r.string_opt("authors")?,
        narrators: r.string_opt("narrators")?,
        description: r.string_opt("description")?,
        subjects: r.string_opt("subjects")?,
        categories: r.string_opt("categories")?,
        language: r.string_opt("language")?,
        series: r.string_opt("series")?,
        series_index: r.string_opt("series_index")?,
        cover_url: r.string_opt("cover_url")?,
        openlibrary_id: r.string_opt("openlibrary_id")?,
        created_at: parse_dt(&r.string_req("created_at")?),
        updated_at: parse_dt(&r.string_req("updated_at")?),
    })
}

fn map_listening_row(r: &Row) -> Result<ListeningProgressRecord> {
    Ok(ListeningProgressRecord {
        id: r.i64_req("id")?,
        identity_id: r.i64_opt("identity_id")?,
        provider: r.string_req("provider")?,
        external_user_id: r.string_req("external_user_id")?,
        book_uuid: r.string_opt("book_uuid")?,
        work_id: r.string_opt("work_id")?,
        external_item_id: r.string_req("external_item_id")?,
        title: r.string_opt("title")?,
        authors: r.string_opt("authors")?,
        asin: r.string_opt("asin")?,
        isbn: r.string_opt("isbn")?,
        progress: r.f64_opt("progress")?,
        current_time_seconds: r.f64_opt("current_time_seconds")?,
        duration_seconds: r.f64_opt("duration_seconds")?,
        is_finished: r.bool_int("is_finished")?,
        last_listened_at: r.string_opt("last_listened_at")?.as_deref().map(parse_dt),
        updated_at: parse_dt(&r.string_req("updated_at")?),
    })
}

fn map_request_row(r: &Row) -> Result<TitleRequestRecord> {
    let status_raw = r.string_or("status", "open");
    Ok(TitleRequestRecord {
        id: r.i64_req("id")?,
        uuid: r.string_req("uuid")?,
        identity_id: r.i64_opt("identity_id")?,
        title: r.string_req("title")?,
        authors: r.string_opt("authors")?,
        asin: r.string_opt("asin")?,
        isbn: r.string_opt("isbn")?,
        notes: r.string_opt("notes")?,
        status: RequestStatus::parse(&status_raw).unwrap_or_default(),
        work_key: r.string_opt("work_key")?.unwrap_or_default(),
        work_id: r.string_opt("work_id")?,
        resolved_book_uuid: r.string_opt("resolved_book_uuid")?,
        created_at: parse_dt(&r.string_req("created_at")?),
        updated_at: parse_dt(&r.string_req("updated_at")?),
    })
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
        assert_eq!(acct.source, "audible");

        let mut book = NewBook::minimal("B00TEST", "user-1", "us", "Test Book");
        book.authors = Some("Author".into());
        let book = store.upsert_book(&book).unwrap();
        assert_eq!(book.title, "Test Book");
        assert!(!book.uuid.is_empty());
        assert_eq!(book.product_id, "B00TEST");
        assert_eq!(book.asin.as_deref(), Some("B00TEST"));
        assert!(book.isbn.is_none());
        assert_eq!(book.source, "audible");
        assert_eq!(book.title_id(), book.uuid.as_str());
        assert_eq!(book.asin_or_isbn(), "B00TEST");
        assert_eq!(book.acquire_status, AcquireStatus::NotAcquired);

        store
            .set_acquire_status(
                "B00TEST",
                "user-1",
                AcquireStatus::Acquired,
                Some("Author/Test Book/book.m4b"),
                None,
            )
            .unwrap();

        let updated = store.get_book("B00TEST", "user-1").unwrap().unwrap();
        assert_eq!(updated.acquire_status, AcquireStatus::Acquired);
        assert_eq!(
            updated.storage_key.as_deref(),
            Some("Author/Test Book/book.m4b")
        );
        assert_eq!(store.count_by_status(AcquireStatus::Acquired).unwrap(), 1);

        let by_uuid = store.get_book_by_uuid(&updated.uuid).unwrap().unwrap();
        assert_eq!(by_uuid.product_id, "B00TEST");
    }

    #[test]
    fn same_isbn_multi_account_and_source() {
        let store = LibraryStore::open_in_memory().unwrap();
        store.upsert_account("user-1", "us", None, true).unwrap();
        store.upsert_account("user-2", "us", None, true).unwrap();
        store
            .upsert_account_with_source("libro-1", "us", None, true, "libro")
            .unwrap();

        let mut a1 = NewBook::minimal("B00SAME", "user-1", "us", "Same Book");
        a1.isbn = Some("9781234567890".into());
        store.upsert_book(&a1).unwrap();

        let mut a2 = NewBook::minimal("B00SAME", "user-2", "us", "Same Book");
        a2.isbn = Some("9781234567890".into());
        store.upsert_book(&a2).unwrap();

        let libro = NewBook {
            uuid: None,
            product_id: "9781234567890".into(),
            source: "libro".into(),
            account_id: "libro-1".into(),
            asin: None,
            isbn: Some("9781234567890".into()),
            marketplace: "us".into(),
            title: "Same Book".into(),
            authors: None,
            narrators: None,
            series: None,
            series_index: None,
            series_asin: None,
            purchased_at: None,
            publisher: None,
            length_minutes: None,
            is_abridged: false,
            content_kind: "book".into(),
            categories: None,
            subtitle: None,
            published_at: None,
        };
        store.upsert_book(&libro).unwrap();

        let by_isbn = store.find_books_by_isbn("9781234567890").unwrap();
        assert_eq!(by_isbn.len(), 3);
        let uuids: std::collections::HashSet<_> = by_isbn.iter().map(|b| b.uuid.as_str()).collect();
        assert_eq!(uuids.len(), 3);

        let preferred = prefer_enrichment_source(&by_isbn).unwrap();
        assert_eq!(preferred.source, "audible");
    }

    #[test]
    fn libro_rescan_preserves_audible_enrichment() {
        let store = LibraryStore::open_in_memory().unwrap();
        store
            .upsert_account_with_source("libro-1", "us", None, true, "libro")
            .unwrap();

        let isbn = "9781234567890";
        let initial = NewBook {
            uuid: None,
            product_id: isbn.into(),
            source: "libro".into(),
            account_id: "libro-1".into(),
            asin: None,
            isbn: Some(isbn.into()),
            marketplace: "us".into(),
            title: "Sparse Libro Title".into(),
            authors: Some("Libro Author".into()),
            narrators: None,
            series: None,
            series_index: None,
            series_asin: None,
            purchased_at: None,
            publisher: None,
            length_minutes: None,
            is_abridged: false,
            content_kind: "book".into(),
            categories: None,
            subtitle: None,
            published_at: None,
        };
        let row = store.upsert_book(&initial).unwrap();

        // Simulate Audible catalog enrichment.
        let enriched = NewBook {
            uuid: Some(row.uuid.clone()),
            asin: Some("B00ENRICHED".into()),
            title: "Rich Audible Title".into(),
            authors: Some("Audible Author".into()),
            narrators: Some("Audible Narrator".into()),
            series: Some("Audible Series".into()),
            publisher: Some("Publisher".into()),
            length_minutes: Some(420),
            subtitle: Some("A Subtitle".into()),
            ..initial.clone()
        };
        let after_enrich = store.upsert_book(&enriched).unwrap();
        assert_eq!(after_enrich.asin.as_deref(), Some("B00ENRICHED"));
        assert_eq!(after_enrich.title, "Rich Audible Title");
        assert_eq!(after_enrich.narrators.as_deref(), Some("Audible Narrator"));

        // Libro rescan without asin must not wipe enrichment.
        let rescan = NewBook {
            title: "Sparse Libro Title Again".into(),
            authors: Some("Libro Author".into()),
            narrators: None,
            asin: None,
            series: None,
            publisher: None,
            length_minutes: Some(400),
            subtitle: None,
            ..initial
        };
        let after_rescan = store.upsert_book(&rescan).unwrap();
        assert_eq!(after_rescan.uuid, row.uuid);
        assert_eq!(after_rescan.asin.as_deref(), Some("B00ENRICHED"));
        assert_eq!(after_rescan.title, "Rich Audible Title");
        assert_eq!(after_rescan.authors.as_deref(), Some("Audible Author"));
        assert_eq!(after_rescan.narrators.as_deref(), Some("Audible Narrator"));
        assert_eq!(after_rescan.series.as_deref(), Some("Audible Series"));
        assert_eq!(after_rescan.publisher.as_deref(), Some("Publisher"));
        assert_eq!(after_rescan.length_minutes, Some(420));
        assert_eq!(after_rescan.subtitle.as_deref(), Some("A Subtitle"));
        assert_eq!(after_rescan.download_product_id(), isbn);
    }

    #[test]
    fn download_product_id_is_source_native() {
        let store = LibraryStore::open_in_memory().unwrap();
        store
            .upsert_account_with_source("libro-1", "us", None, true, "libro")
            .unwrap();
        let book = store
            .upsert_book(&NewBook {
                uuid: None,
                product_id: "9789999999999".into(),
                source: "libro".into(),
                account_id: "libro-1".into(),
                asin: Some("B00FROMAD".into()),
                isbn: Some("9789999999999".into()),
                marketplace: "us".into(),
                title: "Enriched".into(),
                authors: None,
                narrators: None,
                series: None,
                series_index: None,
                series_asin: None,
                purchased_at: None,
                publisher: None,
                length_minutes: None,
                is_abridged: false,
                content_kind: "book".into(),
                categories: None,
                subtitle: None,
                published_at: None,
            })
            .unwrap();
        assert_eq!(book.download_product_id(), "9789999999999");
        assert_eq!(book.audible_asin(), Some("B00FROMAD"));
    }

    #[test]
    fn ensure_account_preserves_scan_enabled() {
        let store = LibraryStore::open_in_memory().unwrap();
        store
            .upsert_account("user-1", "us", Some("Main"), false)
            .unwrap();
        store.ensure_account("user-1", "us", Some("Main")).unwrap();
        let acct = store.get_account("user-1").unwrap().unwrap();
        assert!(!acct.scan_enabled);
    }

    #[test]
    fn upsert_account_with_source_persists() {
        let store = LibraryStore::open_in_memory().unwrap();
        let acct = store
            .upsert_account_with_source("libro-1", "us", Some("Libro"), true, "libro")
            .unwrap();
        assert_eq!(acct.source, "libro");
        let again = store.get_account("libro-1").unwrap().unwrap();
        assert_eq!(again.source, "libro");
    }

    #[test]
    fn remap_account_moves_books() {
        let store = LibraryStore::open_in_memory().unwrap();
        store
            .upsert_account("email@example.com", "us", Some("Main"), true)
            .unwrap();
        store
            .upsert_book(&NewBook::minimal(
                "B00TEST",
                "email@example.com",
                "us",
                "Test Book",
            ))
            .unwrap();

        store
            .remap_account_id("email@example.com", "amzn1.account.CID")
            .unwrap();

        assert!(store.get_account("email@example.com").unwrap().is_none());
        assert!(store.get_account("amzn1.account.CID").unwrap().is_some());
        assert!(store
            .get_book("B00TEST", "amzn1.account.CID")
            .unwrap()
            .is_some());
    }

    #[test]
    fn ignored_titles_roundtrip() {
        let store = LibraryStore::open_in_memory().unwrap();
        store.upsert_account("user-1", "us", None, true).unwrap();
        store
            .upsert_book(&NewBook::minimal("B00TEST", "user-1", "us", "Test"))
            .unwrap();
        assert!(!store.is_ignored("B00TEST", "user-1").unwrap());
        store
            .set_ignored("B00TEST", "user-1", true, Some("skip"))
            .unwrap();
        assert!(store.is_ignored("B00TEST", "user-1").unwrap());
        store.set_ignored("B00TEST", "user-1", false, None).unwrap();
        assert!(!store.is_ignored("B00TEST", "user-1").unwrap());
    }

    #[test]
    fn revoke_keeps_books_and_portal_tickets_work() {
        let store = LibraryStore::open_in_memory().unwrap();
        store
            .upsert_account("user-1", "us", Some("Main"), true)
            .unwrap();
        store
            .upsert_book(&NewBook::minimal("B00TEST", "user-1", "us", "Test"))
            .unwrap();
        store.revoke_credentials("user-1").unwrap();
        let acct = store.get_account("user-1").unwrap().unwrap();
        assert!(!acct.scan_enabled);
        assert_eq!(acct.connection_status, "revoked");
        assert!(store.get_book("B00TEST", "user-1").unwrap().is_some());

        let identity = store
            .upsert_portal_identity("audiobookshelf", "usr_1", Some("bob"))
            .unwrap();
        let ticket = store
            .insert_claim_ticket(
                "abc123hash",
                Some(identity.id),
                Utc::now() + chrono::Duration::hours(1),
                "test",
            )
            .unwrap();
        assert!(ticket.redeemed_at.is_none());
        store.redeem_claim_ticket("abc123hash").unwrap();
        let redeemed = store
            .get_claim_ticket_by_hash("abc123hash")
            .unwrap()
            .unwrap();
        assert!(redeemed.redeemed_at.is_some());

        store
            .link_account(identity.id, "user-1", "audible")
            .unwrap();
        let links = store.list_account_links(identity.id).unwrap();
        assert_eq!(links.len(), 1);
        store.mark_connection_active("user-1").unwrap();
        assert_eq!(
            store
                .get_account("user-1")
                .unwrap()
                .unwrap()
                .connection_status,
            "active"
        );
    }

    #[test]
    fn user_preferences_roundtrip_operator_and_portal() {
        use crate::models::{portal_prefs_key, OPERATOR_PREFS_KEY};

        let store = LibraryStore::open_in_memory().unwrap();
        let defaults = store
            .get_user_preferences_or_default(OPERATOR_PREFS_KEY, None)
            .unwrap();
        assert_eq!(defaults.default_view, "discover");
        assert!(defaults.disabled_shelves.is_empty());
        assert!(store
            .get_user_preferences(OPERATOR_PREFS_KEY)
            .unwrap()
            .is_none());

        let saved = store
            .upsert_user_preferences(
                OPERATOR_PREFS_KEY,
                None,
                "library",
                &["chirp_deals".into(), "genre".into()],
            )
            .unwrap();
        assert_eq!(saved.default_view, "library");
        assert_eq!(saved.disabled_shelves, vec!["chirp_deals", "genre"]);

        let again = store
            .get_user_preferences(OPERATOR_PREFS_KEY)
            .unwrap()
            .unwrap();
        assert_eq!(again.default_view, "library");
        assert_eq!(again.disabled_shelves.len(), 2);

        let identity = store
            .upsert_portal_identity("audiobookshelf", "usr_prefs", Some("alice"))
            .unwrap();
        let key = portal_prefs_key(identity.id);
        let portal = store
            .upsert_user_preferences(&key, Some(identity.id), "accounts", &["narrator".into()])
            .unwrap();
        assert_eq!(portal.identity_id, Some(identity.id));
        assert_eq!(portal.default_view, "accounts");
        assert_eq!(portal.disabled_shelves, vec!["narrator"]);

        // Operator prefs stay independent.
        assert_eq!(
            store
                .get_user_preferences(OPERATOR_PREFS_KEY)
                .unwrap()
                .unwrap()
                .default_view,
            "library"
        );
    }

    #[test]
    fn wishlist_is_personal_and_global_queue_ranks_by_wish_count() {
        let store = LibraryStore::open_in_memory().unwrap();
        let a = store
            .upsert_portal_identity("audiobookshelf", "u1", Some("alice"))
            .unwrap();
        let b = store
            .upsert_portal_identity("audiobookshelf", "u2", Some("bob"))
            .unwrap();

        let work = fallback_work_key("Hail Mary", Some("Andy Weir"), Some("B00HAIL"), None);
        store
            .create_title_request(&NewTitleRequest {
                uuid: None,
                identity_id: Some(a.id),
                title: "Hail Mary".into(),
                authors: Some("Andy Weir".into()),
                asin: Some("B00HAIL".into()),
                isbn: None,
                notes: None,
                status: RequestStatus::Open, // ignored
                work_key: work.clone(),
                work_id: None,
                resolved_book_uuid: None,
            })
            .unwrap();
        store
            .create_title_request(&NewTitleRequest {
                uuid: None,
                identity_id: Some(b.id),
                title: "Project Hail Mary".into(),
                authors: Some("Andy Weir".into()),
                asin: Some("B00HAIL".into()),
                isbn: None,
                notes: None,
                status: RequestStatus::Open,
                work_key: work.clone(),
                work_id: None,
                resolved_book_uuid: None,
            })
            .unwrap();
        // Solo wish — should rank below Hail Mary.
        store
            .create_title_request(&NewTitleRequest {
                uuid: None,
                identity_id: None,
                title: "Solo Title".into(),
                authors: None,
                asin: Some("B00SOLO".into()),
                isbn: None,
                notes: None,
                status: RequestStatus::Open,
                work_key: String::new(),
                work_id: None,
                resolved_book_uuid: None,
            })
            .unwrap();

        // Idempotent for same identity + work.
        let again = store
            .create_title_request(&NewTitleRequest {
                uuid: None,
                identity_id: Some(a.id),
                title: "Hail Mary".into(),
                authors: Some("Andy Weir".into()),
                asin: Some("B00HAIL".into()),
                isbn: None,
                notes: None,
                status: RequestStatus::Open,
                work_key: work.clone(),
                work_id: None,
                resolved_book_uuid: None,
            })
            .unwrap();
        assert_eq!(again.asin.as_deref(), Some("B00HAIL"));
        assert_eq!(store.list_wishlist(Some(a.id)).unwrap().len(), 1);
        assert_eq!(store.list_wishlist(Some(b.id)).unwrap().len(), 1);

        // Soft catalog key vs later asin: for the same wisher → one open row.
        let soft = store
            .create_title_request(&NewTitleRequest {
                uuid: None,
                identity_id: Some(a.id),
                title: "The Martian".into(),
                authors: Some("Andy Weir".into()),
                asin: None,
                isbn: None,
                notes: None,
                status: RequestStatus::Open,
                work_key: String::from("soft:the martian|andy weir"),
                work_id: None,
                resolved_book_uuid: None,
            })
            .unwrap();
        let again_hard = store
            .create_title_request(&NewTitleRequest {
                uuid: None,
                identity_id: Some(a.id),
                title: "The Martian".into(),
                authors: Some("Andy Weir".into()),
                asin: Some("B00MARTIAN".into()),
                isbn: None,
                notes: None,
                status: RequestStatus::Open,
                work_key: String::from("asin:B00MARTIAN"),
                work_id: None,
                resolved_book_uuid: None,
            })
            .unwrap();
        assert_eq!(soft.uuid, again_hard.uuid);
        assert_eq!(store.list_wishlist(Some(a.id)).unwrap().len(), 2);
        assert_eq!(store.list_wishlist(None).unwrap().len(), 1);

        let queue = store.list_global_request_queue().unwrap();
        // Hail Mary (2 wishes) ranks above Solo + Martian (1 each).
        assert_eq!(queue.len(), 3);
        assert_eq!(queue[0].wish_count, 2);
        assert_eq!(queue[0].work_key, work);
        assert!(queue
            .iter()
            .any(|e| e.wish_count == 1 && e.title.contains("Martian")));
        assert!(queue
            .iter()
            .any(|e| e.wish_count == 1 && e.title.contains("Solo")));
    }
}
