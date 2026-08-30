//! Host-private resolved SQL v1 statement proof.
//!
//! Canonical [`crate::ExecuteRequest`] stays the public wire. Admission builds
//! one [`ResolvedStatement`] per statement; authorization, schema companions,
//! and adapter lowering consume it. This module is not a guest SDK surface.

use super::{statement_sql_hash, SqlType};

/// Byte span in the exact canonical SQL string a proof is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqlSpan {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

/// One TEXT expression that PostgreSQL must collate as `"C"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCollateSite {
    /// Identifier or string-literal span in canonical SQL.
    pub span: SqlSpan,
}

/// Physical table/column access used for authorization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalAccess {
    /// Folded physical table name.
    pub table: String,
    /// Folded column name when the access is column-specific.
    pub column: Option<String>,
}

/// Destination assignment `lhs = rhs` (INSERT, UPDATE, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssignment {
    /// Physical table of the destination column.
    pub table: String,
    /// Destination column.
    pub column: String,
    /// Destination type.
    pub dest: SqlType,
    /// Unified source type.
    pub source: SqlType,
}

/// CREATE/DROP action recorded on a proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaAction {
    /// Not DDL.
    None,
    /// `CREATE TABLE` — `noop` when the durable fingerprint already matches.
    Create {
        /// Folded table name.
        table: String,
        /// Structured schema fingerprint (hex SHA-256).
        fingerprint: String,
        /// Identity column, if any.
        identity_column: Option<String>,
        /// True when catalog already has this exact fingerprint.
        noop: bool,
    },
    /// `DROP TABLE`.
    Drop {
        /// Folded table name.
        table: String,
    },
}

/// One resolved typed proof bound to an exact canonical statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStatement {
    /// SHA-256 hex of the exact canonical SQL this proof claims.
    pub statement_hash: String,
    /// SELECT / RETURNING / VALUES output columns in order.
    pub output_columns: Vec<(String, SqlType)>,
    /// Physical tables/columns referenced (authorization).
    pub physical_accesses: Vec<PhysicalAccess>,
    /// Mutation destination assignments.
    pub assignments: Vec<ResolvedAssignment>,
    /// TEXT expression spans for `COLLATE "C"` (already excluding dest/FROM/AS).
    pub text_collate_sites: Vec<TextCollateSite>,
    /// DDL action + fingerprint.
    pub schema_action: SchemaAction,
}

impl ResolvedStatement {
    /// Proof for `sql` with empty semantic payloads (wrapper DDL/DML still hash-bound).
    #[must_use]
    pub fn bound_empty(sql: &str) -> Self {
        Self {
            statement_hash: statement_sql_hash(sql),
            output_columns: Vec::new(),
            physical_accesses: Vec::new(),
            assignments: Vec::new(),
            text_collate_sites: Vec::new(),
            schema_action: SchemaAction::None,
        }
    }

    /// True when `sql` is the exact statement this proof was built from.
    #[must_use]
    pub fn proves(&self, sql: &str) -> bool {
        self.statement_hash == statement_sql_hash(sql)
    }
}

/// Require `proof` to be bound to `sql`.
///
/// # Errors
///
/// Returns when the hashes differ (transformed or reindexed SQL).
pub fn assert_proof_matches_sql(proof: &ResolvedStatement, sql: &str) -> crate::Result<()> {
    if proof.proves(sql) {
        Ok(())
    } else {
        Err(crate::PluginError::internal(
            "resolved SQL proof is not bound to this canonical statement",
        ))
    }
}
