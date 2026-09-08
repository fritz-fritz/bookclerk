//! Plugin-owned database migration registration DTOs (Cap'n Proto `databaseMigrations`).
//!
//! `id` is an opaque plugin-chosen identity. Bookclerk assigns no version,
//! order, or predecessor meaning to it. Registration order is the sequence.
//! Transport/resource limits are enforced by
//! [`require_plugin_migration_registration`] before semantic SQL proof.

use serde::{Deserialize, Serialize};

use crate::{
    PluginError, Result, MAX_LIST_PAGE, MAX_PLUGIN_MIGRATION_OPS,
    MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES, MAX_SCALAR_BYTES,
};

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

/// Converts a Rust list length into a Cap'n Proto `UInt32` list size.
///
/// # Errors
///
/// Returns [`PluginError::payload_too_large`] when `len` cannot fit in `u32`.
pub fn capnp_u32_len(len: usize, what: &str) -> Result<u32> {
    u32::try_from(len).map_err(|_| {
        PluginError::payload_too_large(format!("{what} length {len} exceeds Cap'n Proto UInt32"))
    })
}

/// UTF-8 bytes of one migration's id plus every operation's SQL (no wire framing).
#[must_use]
pub fn plugin_migration_bytes(migration: &PluginMigration) -> usize {
    migration.id.len()
        + migration
            .operations
            .iter()
            .map(|op| op.sql().len())
            .sum::<usize>()
}

/// Aggregate UTF-8 bytes of ids plus SQL across a complete registration.
#[must_use]
pub fn plugin_migration_registration_bytes(migrations: &[PluginMigration]) -> usize {
    migrations.iter().map(plugin_migration_bytes).sum()
}

/// Converts a product `UInt32` constant into a `usize` limit.
fn usize_limit(max: u32) -> usize {
    usize::try_from(max).unwrap_or(0)
}

/// Rejects a plugin migration registration that exceeds RPC resource limits.
///
/// Call this before encoding a Cap'n Proto / JSON reply and while decoding,
/// before semantic BookclerkSQL proof. Limits:
///
/// - migration count: [`MAX_LIST_PAGE`]
/// - operations per migration: [`MAX_PLUGIN_MIGRATION_OPS`]
/// - each SQL text: [`MAX_SCALAR_BYTES`]
/// - aggregate id + SQL bytes: [`MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES`]
///
/// # Errors
///
/// Returns [`PluginError::payload_too_large`] when any limit is exceeded.
pub fn require_plugin_migration_registration(migrations: &[PluginMigration]) -> Result<()> {
    let max_count = usize_limit(MAX_LIST_PAGE);
    if migrations.len() > max_count {
        return Err(PluginError::payload_too_large(format!(
            "plugin migration count {} exceeds maxListPage ({max_count})",
            migrations.len()
        )));
    }
    let max_ops = usize_limit(MAX_PLUGIN_MIGRATION_OPS);
    let max_sql = usize_limit(MAX_SCALAR_BYTES);
    let max_reg = usize_limit(MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES);
    let mut total = 0usize;
    for migration in migrations {
        if migration.operations.len() > max_ops {
            return Err(PluginError::payload_too_large(format!(
                "plugin migration `{}` has {} operations; exceeds maxPluginMigrationOps ({max_ops})",
                migration.id,
                migration.operations.len()
            )));
        }
        total = total.saturating_add(migration.id.len());
        if total > max_reg {
            return Err(PluginError::payload_too_large(format!(
                "plugin migration registration is {total} bytes; exceeds \
                 maxPluginMigrationRegistrationBytes ({max_reg})"
            )));
        }
        for op in &migration.operations {
            let n = op.sql().len();
            if n > max_sql {
                return Err(PluginError::payload_too_large(format!(
                    "plugin migration `{}` SQL is {n} bytes; exceeds maxScalarBytes ({max_sql})",
                    migration.id
                )));
            }
            total = total.saturating_add(n);
            if total > max_reg {
                return Err(PluginError::payload_too_large(format!(
                    "plugin migration registration is {total} bytes; exceeds \
                     maxPluginMigrationRegistrationBytes ({max_reg})"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;
    use crate::PluginErrorCode;

    fn schema_mig(id: &str, sql: impl Into<String>) -> PluginMigration {
        PluginMigration {
            id: id.into(),
            operations: vec![PluginMigrationOp::Schema(sql.into())],
        }
    }

    fn n_migrations(n: usize) -> Vec<PluginMigration> {
        (0..n)
            .map(|i| schema_mig(&format!("m{i:03}"), "x"))
            .collect()
    }

    fn n_ops(n: usize) -> PluginMigration {
        PluginMigration {
            id: "ops".into(),
            operations: (0..n)
                .map(|_| PluginMigrationOp::Schema("x".into()))
                .collect(),
        }
    }

    fn too_large(err: &PluginError) {
        assert_eq!(err.code, PluginErrorCode::PayloadTooLarge, "{err}");
    }

    #[test]
    fn max_migration_count_is_accepted() {
        let migrations = n_migrations(usize_limit(MAX_LIST_PAGE));
        require_plugin_migration_registration(&migrations).expect("count N");
        assert_eq!(migrations.len(), usize_limit(MAX_LIST_PAGE));
    }

    #[test]
    fn migration_count_plus_one_is_payload_too_large() {
        let err =
            require_plugin_migration_registration(&n_migrations(usize_limit(MAX_LIST_PAGE) + 1))
                .expect_err("count N+1");
        too_large(&err);
        assert!(err.message.contains("maxListPage"), "{err}");
    }

    #[test]
    fn max_ops_in_one_migration_is_accepted() {
        require_plugin_migration_registration(&[n_ops(usize_limit(MAX_PLUGIN_MIGRATION_OPS))])
            .expect("ops N");
    }

    #[test]
    fn ops_plus_one_is_payload_too_large() {
        let err = require_plugin_migration_registration(&[n_ops(
            usize_limit(MAX_PLUGIN_MIGRATION_OPS) + 1,
        )])
        .expect_err("ops N+1");
        too_large(&err);
        assert!(err.message.contains("maxPluginMigrationOps"), "{err}");
    }

    #[test]
    fn max_individual_sql_is_accepted() {
        let sql = "x".repeat(usize_limit(MAX_SCALAR_BYTES));
        // Aggregate counts id bytes; an empty id is the only way to also sit
        // at maxScalarBytes without exceeding maxPluginMigrationRegistrationBytes.
        require_plugin_migration_registration(&[schema_mig("", sql)]).expect("sql N");
    }

    #[test]
    fn individual_sql_plus_one_is_payload_too_large() {
        let sql = "x".repeat(usize_limit(MAX_SCALAR_BYTES) + 1);
        let err =
            require_plugin_migration_registration(&[schema_mig("sql", sql)]).expect_err("sql N+1");
        too_large(&err);
        assert!(err.message.contains("maxScalarBytes"), "{err}");
    }

    #[test]
    fn max_aggregate_registration_is_accepted() {
        let max_reg = usize_limit(MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES);
        let id = "a";
        let sql = "x".repeat(max_reg - id.len());
        require_plugin_migration_registration(&[schema_mig(id, sql)]).expect("aggregate N");
    }

    #[test]
    fn aggregate_plus_one_is_payload_too_large_when_each_sql_fits() {
        let half = usize_limit(MAX_SCALAR_BYTES) / 2 + 1;
        assert!(half <= usize_limit(MAX_SCALAR_BYTES));
        let sql = "x".repeat(half);
        let migrations = vec![schema_mig("a", sql.clone()), schema_mig("b", sql)];
        let total = plugin_migration_registration_bytes(&migrations);
        assert!(total > usize_limit(MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES));
        let err = require_plugin_migration_registration(&migrations).expect_err("aggregate N+1");
        too_large(&err);
        assert!(
            err.message.contains("maxPluginMigrationRegistrationBytes"),
            "{err}"
        );
    }

    #[test]
    fn capnp_u32_len_rejects_lengths_that_cannot_fit() {
        let len = usize::try_from(u32::MAX)
            .expect("u32::MAX fits usize")
            .saturating_add(1);
        let err = capnp_u32_len(len, "plugin migrations").expect_err("u32 overflow");
        too_large(&err);
        assert!(err.message.contains("UInt32"), "{err}");
        assert_eq!(
            capnp_u32_len(usize_limit(MAX_LIST_PAGE), "plugin migrations").expect("fits"),
            MAX_LIST_PAGE
        );
    }
}
