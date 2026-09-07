//! Plugin-owned database migration registration DTOs (Cap'n Proto `databaseMigrations`).
//!
//! `id` is an opaque plugin-chosen identity. Bookclerk assigns no version,
//! order, or predecessor meaning to it. Registration order is the sequence.

use serde::{Deserialize, Serialize};

/// Maximum UTF-8 bytes of one plugin-chosen migration id.
pub const MAX_PLUGIN_MIGRATION_ID_BYTES: usize = 128;

/// Host-private plugin-binding migration journal (not a plugin-owned table).
pub const PLUGIN_MIGRATIONS_TABLE: &str = "plugin_migrations";

/// One already-separated BookclerkSQL operation in a registered migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginMigrationOp {
    /// Admitted schema DDL (`CREATE` / `DROP` / index).
    Schema(String),
    /// Admitted data DML (`INSERT` / `UPDATE` / `DELETE`).
    Data(String),
}

impl PluginMigrationOp {
    /// Canonical BookclerkSQL for this operation.
    #[must_use]
    pub fn sql(&self) -> &str {
        match self {
            Self::Schema(sql) | Self::Data(sql) => sql,
        }
    }

    /// True when this operation is classified as schema DDL.
    #[must_use]
    pub fn is_schema(&self) -> bool {
        matches!(self, Self::Schema(_))
    }
}

/// One plugin-owned migration application in registration order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMigration {
    /// Opaque plugin-chosen stable identity.
    pub id: String,
    /// Ordered already-separated BookclerkSQL operations.
    pub operations: Vec<PluginMigrationOp>,
}
