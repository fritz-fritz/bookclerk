//! Adapter-edge classification of SeaORM [`DbErr`] into contract categories.
//!
//! Guests already stamp stable tokens (`SQLITE_BUSY`, SQLSTATE `40P01`,
//! `unavailable:`). Host schema apply must not parse engine prose.

use sea_orm::DbErr;

/// Portable Bookclerk SQL error category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbErrorClass {
    /// Retry the same operation after re-reading durable state.
    Unavailable,
    /// Uniqueness / already-exists; re-read state, do not blindly retry DDL.
    Conflict,
    /// Permanent or unclassified failure.
    Other,
}

/// Classifies `err` from adapter-stamped codes, not English engine messages.
#[must_use]
pub fn classify_db_err(err: &DbErr) -> DbErrorClass {
    classify_db_err_message(&err.to_string())
}

/// Classifies an already-formatted adapter/ORM error string.
#[must_use]
pub fn classify_db_err_message(message: &str) -> DbErrorClass {
    let upper = message.to_ascii_uppercase();
    if upper.contains("UNAVAILABLE:")
        || upper.contains("SQLITE_BUSY")
        || upper.contains("SQLITE_LOCKED")
        || upper.contains("40P01")
        || upper.contains("40001")
        || upper.contains("55P03")
        || message.contains("database commit failed")
    {
        return DbErrorClass::Unavailable;
    }
    if upper.contains("SQLITE_CONSTRAINT") || upper.contains("23505") {
        return DbErrorClass::Conflict;
    }
    DbErrorClass::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_adapter_code_tokens() {
        assert_eq!(
            classify_db_err_message("SQLITE_BUSY (5): database is locked"),
            DbErrorClass::Unavailable
        );
        assert_eq!(
            classify_db_err_message("SQLITE_LOCKED (6)"),
            DbErrorClass::Unavailable
        );
        assert_eq!(
            classify_db_err_message("error 40P01 deadlock detected"),
            DbErrorClass::Unavailable
        );
        assert_eq!(
            classify_db_err_message("unavailable: D1 ambiguous commit"),
            DbErrorClass::Unavailable
        );
        assert_eq!(
            classify_db_err_message("SQLITE_CONSTRAINT (2067): UNIQUE"),
            DbErrorClass::Conflict
        );
        assert_eq!(
            classify_db_err_message("duplicate key value violates unique constraint 23505"),
            DbErrorClass::Conflict
        );
        assert_eq!(
            classify_db_err_message("cancelled: atomic session cancelled"),
            DbErrorClass::Other
        );
        assert_eq!(
            classify_db_err_message("syntax error near TABLE"),
            DbErrorClass::Other
        );
    }
}
