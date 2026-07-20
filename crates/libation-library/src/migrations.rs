//! Schema migrations via `rusqlite_migration` (`PRAGMA user_version`).

use rusqlite_migration::{M, Migrations};

/// Ordered schema migrations for the Libation library DB.
///
/// Add new `M::up(...)` entries at the end only — never reorder or edit applied ones.
#[must_use]
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(
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
    )])
}
