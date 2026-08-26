//! Typed Cap'n database data-plane (`ExecuteRequest` / `ExecuteReply`) and
//! control-plane (`DbCapabilities`) mirrors of `plugin.capnp`.
//!
//! Hosts call `DatabaseSession.capabilities` and
//! `DatabaseSession.executeAtomic`. The `bookclerk.capabilities` /
//! `bookclerk.atomic` sentinels route these calls through the SeaORM proxy.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::db_value::{DbType, DbValue};
use crate::MAX_SCALAR_BYTES;

/// SQLite family bind cap advertised by the platform sqlite guest.
pub const SQLITE_MAX_BINDS: u32 = 32_766;

/// PostgreSQL bind cap advertised by the optional postgres guest.
pub const POSTGRES_MAX_BINDS: u32 = 65_535;

/// Cloudflare D1 bound-parameter limit.
///
/// <https://developers.cloudflare.com/d1/platform/limits/>
pub const D1_MAX_BINDS: u32 = 100;

/// D1 / first-party batch statement cap (D1 HTTP batch is 100 queries).
pub const FIRST_PARTY_MAX_STATEMENTS: u32 = 100;

/// First-party row cap advertised for one query statement.
pub const FIRST_PARTY_MAX_RESULT_ROWS: u32 = 1_000;

/// Host refuses guests that cannot bind at least this many parameters.
pub const HOST_MIN_BINDS: u32 = 32;

/// Host refuses guests that cannot run at least this many statements per batch.
pub const HOST_MIN_STATEMENTS: u32 = 40;

/// Host refuses guests that do not bound result rows (`0` is unspecified).
pub const HOST_MIN_RESULT_ROWS: u32 = 1;

/// Host refuses guests that do not bound encoded statement payload bytes.
pub const HOST_MIN_PAYLOAD_BYTES: u32 = 1024;

/// Host refuses guests that do not bound JSON bytes of one statement's rows.
pub const HOST_MIN_RESULT_BYTES: u32 = 4_096;

/// Host refuses guests that do not bound one result cell (`0` is unspecified).
pub const HOST_MIN_CELL_BYTES: u32 = 1_024;

/// First-party JSON-byte budget for one statement's rows and for one atomic
/// request/result scalar. Must stay at or below [`crate::MAX_SCALAR_BYTES`].
pub const FIRST_PARTY_MAX_RESULT_BYTES: u32 = MAX_SCALAR_BYTES;

/// Bookclerk SQL contract version advertised by first-party adapters.
///
/// Contract versions are **monotonic supersets** (see `docs/sql-contract/v1.md`):
/// every guarantee in version *N* remains valid in *N+1*. Guests advertise the
/// highest version they implement; hosts require
/// `sqlContractVersion >= SQL_CONTRACT_VERSION`. A non-superset change must bump
/// this constant and document a new major contract — do not weaken `>=` into a
/// negotiated range until then.
pub const SQL_CONTRACT_VERSION: u32 = 1;

/// Bootstrap-only SeaORM proxy metadata returned by `AdapterDatabaseSession.bootstrap`.
///
/// Not part of the typed [`DbCapabilities`] plane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DbBootstrap {
    /// SQL family for SeaORM proxy bootstrap (`sqlite` or `postgres`).
    pub sql_family: String,
    /// Engine dialect name (`sqlite`, `postgres`, or `postgresql`).
    pub dialect: String,
}

impl DbBootstrap {
    /// Bootstrap metadata for a sqlite-family connection.
    #[must_use]
    pub fn sqlite() -> Self {
        Self {
            sql_family: "sqlite".into(),
            dialect: "sqlite".into(),
        }
    }

    /// Bootstrap metadata for a postgres-family connection.
    #[must_use]
    pub fn postgres() -> Self {
        Self {
            sql_family: "postgres".into(),
            dialect: "postgres".into(),
        }
    }

    /// SeaORM proxy backend failure from bootstrap metadata (`dialect` / `sqlFamily`).
    #[must_use]
    pub fn backend_failure_reason(&self) -> Option<String> {
        let family = self.sql_family.to_ascii_lowercase();
        if !family.is_empty() {
            if family != "sqlite" && family != "postgres" {
                return Some(format!(
                    "database guest sqlFamily {:?} is not sqlite or postgres (SQL-like backends only)",
                    self.sql_family
                ));
            }
            if !self.dialect.is_empty() && !dialect_matches_sql_family(&self.dialect, &family) {
                return Some(format!(
                    "database guest dialect {:?} does not match sqlFamily {:?}",
                    self.dialect, self.sql_family
                ));
            }
            return None;
        }
        let dialect = self.dialect.to_ascii_lowercase();
        if dialect.is_empty() {
            return Some("database guest dialect is required for SeaORM proxy bootstrap".into());
        }
        if dialect == "sqlite"
            || dialect == "postgres"
            || dialect == "postgresql"
            || dialect == "pg"
        {
            return None;
        }
        Some(format!(
            "database guest dialect {:?} is not sqlite or postgres (SQL-like backends only)",
            self.dialect
        ))
    }
}

/// True when SeaORM `dialect` names the same SQL family as `sql_family`.
fn dialect_matches_sql_family(dialect: &str, sql_family: &str) -> bool {
    match sql_family {
        "sqlite" => dialect.eq_ignore_ascii_case("sqlite"),
        "postgres" => {
            dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql")
        }
        _ => false,
    }
}

/// How a guest should run one statement inside an atomic plan.
///
/// `Select` versus `Returning` is explicit so adapters never reparse SQL to
/// decide whether `SELECT * FROM (…)` wrapping is valid. Matches Cap'n
/// `DbStatementKind` (`execute` | `select` | `returning`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DbPlanStatementKind {
    /// Statement is DML; only `rowsAffected` is required.
    Execute,
    /// Read-only `SELECT` / read-only `WITH` CTE. May be wrapped with `LIMIT`.
    Select,
    /// DML that returns rows (`INSERT`/`UPDATE`/`DELETE … RETURNING`), or
    /// row-producing introspection (`PRAGMA`, schema reads) that must **not**
    /// be rewritten as a subquery.
    #[default]
    Returning,
}

impl DbPlanStatementKind {
    /// True when the guest must collect `rows` (not only `rowsAffected`).
    #[must_use]
    pub const fn collects_rows(self) -> bool {
        !matches!(self, Self::Execute)
    }

    /// True when the guest may wrap SQL as `SELECT * FROM (sql) LIMIT cap+1`.
    #[must_use]
    pub const fn wrap_select_limit(self) -> bool {
        matches!(self, Self::Select)
    }
}

/// How the guest should return results for one statement.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DbResultSelection {
    /// Drop rows and `rowsAffected`.
    Discard,
    /// Return `rowsAffected` only.
    #[default]
    AffectedRows,
    /// Return positional rows plus column metadata.
    Rows,
}

/// One column in a typed result set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbColumn {
    /// Column name.
    pub name: String,
    /// Declared / inferred type.
    pub db_type: DbType,
}

/// One positional result row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbRow {
    /// Cells in column order.
    pub values: Vec<DbValue>,
}

/// One statement in a typed [`ExecuteRequest`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TypedDbStatement {
    /// Canonical Bookclerk SQL (`?` placeholders).
    pub sql: String,
    /// Ordered typed binds.
    pub parameters: Vec<DbValue>,
    /// Host-authored kind (adapters must not reparse SQL).
    pub kind: DbPlanStatementKind,
    /// Proven row upper bound (`0` = unproven).
    pub max_rows: u32,
    /// Which result fields the caller needs.
    pub result_selection: DbResultSelection,
}

/// Typed atomic batch. Every request is a non-empty ordered statement list.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteRequest {
    /// Caller-chosen idempotency key.
    pub operation_id: String,
    /// SHA-256 hex of the idempotency-relevant request; empty when omitted.
    pub request_hash: String,
    /// Ordered statements (must be non-empty).
    pub statements: Vec<TypedDbStatement>,
    /// Guest-visible deadline (unix ms). Zero means omitted.
    pub deadline_unix_ms: u64,
}

/// Result of one statement in [`ExecuteReply`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatementResult {
    /// Positional rows (empty when discarded).
    pub rows: Vec<DbRow>,
    /// Column metadata matching [`Self::rows`] cell order.
    pub columns: Vec<DbColumn>,
    /// Engine-reported rows affected.
    pub rows_affected: u64,
}

impl StatementResult {
    /// Builds a row-bearing result and rejects width / name errors.
    ///
    /// # Errors
    ///
    /// Returns when a row length differs from `columns.len()` or a column name
    /// is duplicated.
    pub fn from_rows(columns: Vec<DbColumn>, rows: Vec<DbRow>) -> Result<Self, String> {
        let stmt = Self {
            rows,
            columns,
            rows_affected: 0,
        };
        stmt.validate_positional()?;
        Ok(stmt)
    }

    /// Builds an affected-rows-only result.
    #[must_use]
    pub fn from_affected(rows_affected: u64) -> Self {
        Self {
            rows: Vec::new(),
            columns: Vec::new(),
            rows_affected,
        }
    }

    /// Rejects duplicate column names and row widths that do not match `columns`.
    ///
    /// # Errors
    ///
    /// Returns when a row has the wrong cell count or two columns share a name.
    pub fn validate_positional(&self) -> Result<(), String> {
        let mut seen = HashSet::with_capacity(self.columns.len());
        for col in &self.columns {
            if !seen.insert(col.name.as_str()) {
                return Err(format!("duplicate result column name `{}`", col.name));
            }
        }
        let width = self.columns.len();
        for (i, row) in self.rows.iter().enumerate() {
            if row.values.len() != width {
                return Err(format!(
                    "result row {i} has {} values; columns has {width}",
                    row.values.len()
                ));
            }
        }
        Ok(())
    }
}

/// Engine timing on [`ExecuteReply`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DbTiming {
    /// Monotonic duration of this handler attempt.
    pub attempt_elapsed_us: u64,
    /// Engine-reported SQL/transaction time when available (`0` = omitted).
    pub db_execution_us: u64,
    /// How `db_execution_us` was measured.
    pub db_timing_source: String,
}

/// Typed atomic reply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteReply {
    /// Echo of the request `operationId`.
    pub operation_id: String,
    /// Per-statement results, in plan order.
    pub statements: Vec<StatementResult>,
    /// Handler/engine timing.
    pub timing: DbTiming,
}

impl ExecuteReply {
    /// Rejects positional errors on every statement result.
    ///
    /// # Errors
    ///
    /// Returns when any statement has a row-width or duplicate-name error.
    pub fn validate_positional(&self) -> Result<(), String> {
        for (i, stmt) in self.statements.iter().enumerate() {
            stmt.validate_positional()
                .map_err(|err| format!("statement {i}: {err}"))?;
        }
        Ok(())
    }
}

/// Semantic SQL-contract advertisement (`DatabaseSession.capabilities`).
///
/// Bootstrap metadata (`sql_family`, `diagnostic_engine`, SeaORM `dialect`) is
/// negotiated separately via [`DbBootstrap`] — not on this typed capability plane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbCapabilities {
    /// Bookclerk SQL contract version.
    pub sql_contract_version: u32,
    /// Guest can run a bounded statement list as one SQL transaction.
    pub atomic_batch: bool,
    /// Guest SQL supports `RETURNING`.
    pub returning: bool,
    /// Guest reports `rowsAffected`.
    pub affected_rows: bool,
    /// Guest versions schema with a `schema_migrations` table.
    pub schema_migrations: bool,
    /// Guest versions schema with `PRAGMA user_version`.
    pub pragma_user_version: bool,
    /// Each schema version must be applied as one atomic batch.
    pub atomic_schema_batch: bool,
    /// Guest honors RPC/session cancellation.
    pub cancellation: bool,
    /// Guest can fill [`DbTiming::db_execution_us`].
    pub timing: bool,
    /// Maximum bound parameters per statement.
    pub max_binds: u32,
    /// Maximum statements in one atomic batch.
    pub max_statements: u32,
    /// Maximum rows a query statement may return.
    pub max_result_rows: u32,
    /// Maximum UTF-8 bytes of SQL plus binds per statement.
    pub max_payload_bytes: u32,
    /// Maximum encoded bytes of one statement's result rows.
    pub max_result_bytes: u32,
    /// Maximum UTF-8 / blob bytes of one result cell.
    pub max_cell_bytes: u32,
    /// Maximum encoded bytes of one [`ExecuteRequest`].
    pub max_request_bytes: u32,
    /// Maximum encoded bytes of one [`ExecuteReply`].
    pub max_atomic_result_bytes: u32,
    /// Adapter can open additional isolated sessions for plugin-owned
    /// database bindings (per-binding file / schema / database).
    #[serde(default)]
    pub plugin_databases: bool,
}

impl DbCapabilities {
    /// True when this guest meets the host's compiled minimum SQL contract.
    #[must_use]
    pub fn meets_host_minimums(&self) -> bool {
        self.capability_failure_reason_opt().is_none()
    }

    /// Operator-facing reason when [`Self::meets_host_minimums`] is false.
    #[must_use]
    pub fn capability_failure_reason(&self) -> String {
        self.capability_failure_reason_opt()
            .unwrap_or_else(|| "database guest failed capability negotiation".into())
    }

    /// Failure reason, or `None` when the guest meets host minima.
    fn capability_failure_reason_opt(&self) -> Option<String> {
        if !self.affected_rows {
            return Some("database guest does not advertise affectedRows".into());
        }
        if !self.cancellation {
            return Some("database guest does not advertise cancellation".into());
        }
        if !self.atomic_batch {
            return Some("database guest does not advertise atomicBatch".into());
        }
        if !self.returning {
            return Some(
                "database guest does not advertise returning (host plans require RETURNING)".into(),
            );
        }
        if self.max_binds < HOST_MIN_BINDS {
            return Some(format!(
                "database guest maxBinds {} is below host minimum {HOST_MIN_BINDS}",
                self.max_binds
            ));
        }
        if self.max_statements < HOST_MIN_STATEMENTS {
            return Some(format!(
                "database guest maxStatements {} is below host minimum {HOST_MIN_STATEMENTS}",
                self.max_statements
            ));
        }
        if self.max_result_rows < HOST_MIN_RESULT_ROWS {
            return Some(format!(
                "database guest maxResultRows {} is below host minimum {HOST_MIN_RESULT_ROWS}",
                self.max_result_rows
            ));
        }
        if self.max_payload_bytes < HOST_MIN_PAYLOAD_BYTES
            || self.max_payload_bytes > MAX_SCALAR_BYTES
        {
            return Some(format!(
                "database guest maxPayloadBytes {} must be between {HOST_MIN_PAYLOAD_BYTES} and {MAX_SCALAR_BYTES}",
                self.max_payload_bytes
            ));
        }
        if self.max_result_bytes < HOST_MIN_RESULT_BYTES {
            return Some(format!(
                "database guest maxResultBytes {} is below host minimum {HOST_MIN_RESULT_BYTES}",
                self.max_result_bytes
            ));
        }
        if self.max_cell_bytes < HOST_MIN_CELL_BYTES {
            return Some(format!(
                "database guest maxCellBytes {} is below host minimum {HOST_MIN_CELL_BYTES}",
                self.max_cell_bytes
            ));
        }
        if self.max_request_bytes < HOST_MIN_RESULT_BYTES
            || self.max_request_bytes > MAX_SCALAR_BYTES
        {
            return Some(format!(
                "database guest maxRequestBytes {} must be between {HOST_MIN_RESULT_BYTES} and {MAX_SCALAR_BYTES}",
                self.max_request_bytes
            ));
        }
        if self.max_atomic_result_bytes < HOST_MIN_RESULT_BYTES
            || self.max_atomic_result_bytes > MAX_SCALAR_BYTES
        {
            return Some(format!(
                "database guest maxAtomicResultBytes {} must be between {HOST_MIN_RESULT_BYTES} and {MAX_SCALAR_BYTES}",
                self.max_atomic_result_bytes
            ));
        }
        if self.max_result_bytes > self.max_atomic_result_bytes {
            return Some(format!(
                "database guest maxResultBytes {} exceeds maxAtomicResultBytes {}",
                self.max_result_bytes, self.max_atomic_result_bytes
            ));
        }
        if self.sql_contract_version < SQL_CONTRACT_VERSION {
            return Some(format!(
                "database guest sqlContractVersion {} is below host minimum {SQL_CONTRACT_VERSION}",
                self.sql_contract_version
            ));
        }
        None
    }

    /// First-party SQLite capability advertisement (`PRAGMA user_version` marker).
    #[must_use]
    pub fn advertised_sqlite() -> Self {
        Self {
            sql_contract_version: SQL_CONTRACT_VERSION,
            atomic_batch: true,
            returning: true,
            affected_rows: true,
            schema_migrations: false,
            pragma_user_version: true,
            atomic_schema_batch: false,
            cancellation: true,
            timing: true,
            max_binds: SQLITE_MAX_BINDS,
            max_statements: FIRST_PARTY_MAX_STATEMENTS,
            max_result_rows: FIRST_PARTY_MAX_RESULT_ROWS,
            max_payload_bytes: MAX_SCALAR_BYTES,
            max_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_cell_bytes: MAX_SCALAR_BYTES,
            max_request_bytes: MAX_SCALAR_BYTES,
            max_atomic_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            plugin_databases: true,
        }
    }

    /// First-party Cloudflare D1 capability advertisement
    /// (`schema_migrations` rows, one atomic HTTP batch per schema version).
    #[must_use]
    pub fn advertised_d1() -> Self {
        Self {
            schema_migrations: true,
            pragma_user_version: false,
            atomic_schema_batch: true,
            max_binds: D1_MAX_BINDS,
            ..Self::advertised_sqlite()
        }
    }

    /// First-party PostgreSQL capability advertisement (`schema_migrations` rows).
    #[must_use]
    pub fn advertised_postgres() -> Self {
        Self {
            schema_migrations: true,
            pragma_user_version: false,
            atomic_schema_batch: false,
            max_binds: POSTGRES_MAX_BINDS,
            ..Self::advertised_sqlite()
        }
    }
}

/// UTF-8 bytes of SQL text plus JSON binds (ordinary query/execute payload).
#[must_use]
pub fn sql_payload_bytes(sql: &str, values_json: &str) -> usize {
    sql.len().saturating_add(values_json.len())
}

/// True when ordinary-path SQL+binds exceed the negotiated payload cap.
///
/// The effective cap is `min(max_payload_bytes, MAX_SCALAR_BYTES)`. A cap of
/// `0` fails closed (any non-empty payload exceeds it).
#[must_use]
pub fn sql_payload_exceeds(sql: &str, values_json: &str, max_payload_bytes: u32) -> bool {
    let cap = usize::try_from(max_payload_bytes.min(MAX_SCALAR_BYTES)).unwrap_or(0);
    sql_payload_bytes(sql, values_json) > cap
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;

    #[test]
    fn payload_cap_is_scalar_ceiling() {
        assert!(sql_payload_exceeds("SELECT 1", "[]", 0));
        assert!(!sql_payload_exceeds("SELECT 1", "[]", 64));
        let big = "x".repeat(MAX_SCALAR_BYTES as usize);
        assert!(sql_payload_exceeds(&big, "[]", MAX_SCALAR_BYTES));
        assert!(!sql_payload_exceeds("SELECT 1", "[]", MAX_SCALAR_BYTES + 1));
    }

    #[test]
    fn capabilities_reject_missing_cancellation() {
        let mut caps = DbCapabilities::advertised_sqlite();
        caps.cancellation = false;
        assert!(!caps.meets_host_minimums());
        assert!(caps.capability_failure_reason().contains("cancellation"));
    }

    #[test]
    fn capabilities_reject_missing_affected_rows() {
        let mut caps = DbCapabilities::advertised_sqlite();
        caps.affected_rows = false;
        assert!(!caps.meets_host_minimums());
        assert!(caps.capability_failure_reason().contains("affectedRows"));
    }

    #[test]
    fn advertised_presets_meet_host_minimums() {
        for caps in [
            DbCapabilities::advertised_sqlite(),
            DbCapabilities::advertised_postgres(),
            DbCapabilities::advertised_d1(),
        ] {
            assert!(
                caps.meets_host_minimums(),
                "{}",
                caps.capability_failure_reason()
            );
            assert_eq!(caps.sql_contract_version, SQL_CONTRACT_VERSION);
            assert_ne!(caps.pragma_user_version, caps.schema_migrations);
        }
        assert_eq!(
            DbCapabilities::advertised_sqlite().max_binds,
            SQLITE_MAX_BINDS
        );
        assert_eq!(
            DbCapabilities::advertised_postgres().max_binds,
            POSTGRES_MAX_BINDS
        );
        assert_eq!(DbCapabilities::advertised_d1().max_binds, D1_MAX_BINDS);
        assert!(DbCapabilities::advertised_d1().atomic_schema_batch);
        assert!(!DbCapabilities::advertised_postgres().atomic_schema_batch);
    }

    #[test]
    fn capabilities_fail_closed_without_returning_or_bounds() {
        let mut no_returning = DbCapabilities::advertised_sqlite();
        no_returning.returning = false;
        assert!(!no_returning.meets_host_minimums());
        assert!(no_returning
            .capability_failure_reason()
            .contains("returning"));

        let mut zero_rows = DbCapabilities::advertised_sqlite();
        zero_rows.max_result_rows = 0;
        assert!(!zero_rows.meets_host_minimums());
        assert!(zero_rows
            .capability_failure_reason()
            .contains("maxResultRows"));

        let mut zero_payload = DbCapabilities::advertised_sqlite();
        zero_payload.max_payload_bytes = 0;
        assert!(!zero_payload.meets_host_minimums());
        assert!(zero_payload
            .capability_failure_reason()
            .contains("maxPayloadBytes"));

        let mut zero_result = DbCapabilities::advertised_sqlite();
        zero_result.max_result_bytes = 0;
        assert!(!zero_result.meets_host_minimums());
        assert!(zero_result
            .capability_failure_reason()
            .contains("maxResultBytes"));

        let mut zero_cell = DbCapabilities::advertised_sqlite();
        zero_cell.max_cell_bytes = 0;
        assert!(!zero_cell.meets_host_minimums());
        assert!(zero_cell
            .capability_failure_reason()
            .contains("maxCellBytes"));

        let mut zero_request = DbCapabilities::advertised_sqlite();
        zero_request.max_request_bytes = 0;
        assert!(!zero_request.meets_host_minimums());
        assert!(zero_request
            .capability_failure_reason()
            .contains("maxRequestBytes"));

        let mut over_scalar = DbCapabilities::advertised_sqlite();
        over_scalar.max_atomic_result_bytes = MAX_SCALAR_BYTES + 1;
        assert!(!over_scalar.meets_host_minimums());
        assert!(over_scalar
            .capability_failure_reason()
            .contains("maxAtomicResultBytes"));
    }

    #[test]
    fn bootstrap_backend_failure_reason_rejects_non_sql_families() {
        let mut bootstrap = DbBootstrap::sqlite();
        bootstrap.sql_family = "mystery".into();
        let reason = bootstrap.backend_failure_reason().expect("reject");
        assert!(reason.contains("sqlFamily"), "{reason}");

        let mut mismatch = DbBootstrap::sqlite();
        mismatch.dialect = "postgres".into();
        let reason = mismatch.backend_failure_reason().expect("reject");
        assert!(reason.contains("does not match"), "{reason}");

        assert!(DbBootstrap::sqlite().backend_failure_reason().is_none());
        assert!(DbBootstrap::postgres().backend_failure_reason().is_none());
        let empty = DbBootstrap::default();
        assert!(empty.backend_failure_reason().is_some());
    }

    #[test]
    fn duplicate_column_names_are_rejected() {
        let stmt = StatementResult {
            columns: vec![
                DbColumn {
                    name: "id".into(),
                    db_type: DbType::Int64,
                },
                DbColumn {
                    name: "id".into(),
                    db_type: DbType::Text,
                },
            ],
            rows: vec![DbRow {
                values: vec![DbValue::Int64(1), DbValue::Text("x".into())],
            }],
            rows_affected: 1,
        };
        let err = stmt.validate_positional().unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[test]
    fn row_width_mismatch_is_rejected() {
        let stmt = StatementResult {
            columns: vec![DbColumn {
                name: "id".into(),
                db_type: DbType::Int64,
            }],
            rows: vec![DbRow {
                values: vec![DbValue::Int64(1), DbValue::Int64(2)],
            }],
            rows_affected: 1,
        };
        let err = stmt.validate_positional().unwrap_err();
        assert!(err.contains("values"), "{err}");
    }

    #[test]
    fn capnp_db_value_goldens_roundtrip() {
        use crate::{decode_db_value_bytes, encoded_db_value_bytes};
        let cases = [
            DbValue::Int64(i64::MIN),
            DbValue::Int64(i64::MAX),
            DbValue::Text("b64:AAAA".into()),
            DbValue::Bytes(vec![0, 1, 2]),
            DbValue::Null(DbType::Bytes),
            DbValue::Boolean(true),
        ];
        for v in cases {
            let bytes = encoded_db_value_bytes(&v).unwrap();
            let back = decode_db_value_bytes(&bytes).unwrap();
            assert_eq!(back, v);
        }
        let text = encoded_db_value_bytes(&DbValue::Text("b64:AAAA".into())).unwrap();
        let blob = encoded_db_value_bytes(&DbValue::Bytes(vec![0, 1, 2])).unwrap();
        assert_ne!(text, blob);
        assert_eq!(
            hex::encode(encoded_db_value_bytes(&DbValue::Int64(i64::MIN)).unwrap()),
            "00000000040000000000000002000100000002000000000000000000000000800000000000000000"
        );
        assert_eq!(
            hex::encode(encoded_db_value_bytes(&DbValue::Int64(i64::MAX)).unwrap()),
            "000000000400000000000000020001000000020000000000ffffffffffffff7f0000000000000000"
        );
        assert_eq!(
            hex::encode(encoded_db_value_bytes(&DbValue::Text("b64:AAAA".into())).unwrap()),
            "0000000006000000000000000200010000000400000000000000000000000000010000004a0000006236343a414141410000000000000000"
        );
        assert_eq!(
            hex::encode(encoded_db_value_bytes(&DbValue::Bytes(vec![0, 1, 2])).unwrap()),
            "0000000005000000000000000200010000000500000000000000000000000000010000001a0000000001020000000000"
        );
        assert_eq!(
            hex::encode(encoded_db_value_bytes(&DbValue::Boolean(true)).unwrap()),
            "00000000040000000000000002000100010001000000000000000000000000000000000000000000"
        );
        assert_eq!(
            hex::encode(encoded_db_value_bytes(&DbValue::Null(DbType::Bytes)).unwrap()),
            "00000000040000000000000002000100050000000000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn execute_request_struct_excludes_host_plan_selectors() {
        let req = ExecuteRequest {
            operation_id: "op".into(),
            request_hash: "abc".into(),
            statements: vec![TypedDbStatement {
                sql: "SELECT 1".into(),
                parameters: vec![],
                kind: DbPlanStatementKind::Select,
                max_rows: 1,
                result_selection: DbResultSelection::Rows,
            }],
            deadline_unix_ms: 0,
        };
        let _ = (
            &req.operation_id,
            &req.request_hash,
            &req.statements,
            req.deadline_unix_ms,
        );
        use crate::{decode_execute_request_bytes, encoded_execute_request_bytes};
        let bytes = encoded_execute_request_bytes(&req).unwrap();
        let back = decode_execute_request_bytes(&bytes).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn capnp_execute_request_roundtrip() {
        use crate::{decode_execute_request_bytes, encoded_execute_request_bytes};
        let req = ExecuteRequest {
            operation_id: "op".into(),
            request_hash: "abc".into(),
            statements: vec![TypedDbStatement {
                sql: "SELECT ?".into(),
                parameters: vec![
                    DbValue::Int64(i64::MIN),
                    DbValue::Text("b64:not-bytes".into()),
                    DbValue::Bytes(vec![0xff]),
                ],
                kind: DbPlanStatementKind::Select,
                max_rows: 1,
                result_selection: DbResultSelection::Rows,
            }],
            deadline_unix_ms: 0,
        };
        let bytes = encoded_execute_request_bytes(&req).unwrap();
        let back = decode_execute_request_bytes(&bytes).unwrap();
        assert_eq!(back.operation_id, req.operation_id);
        assert_eq!(back.statements, req.statements);
    }

    #[test]
    fn public_statement_kind_matches_capnp_ordinals() {
        use crate::{decode_execute_request_bytes, encoded_execute_request_bytes};
        // Cap'n `DbStatementKind`: execute@0, select@1, returning@2.
        for kind in [
            DbPlanStatementKind::Execute,
            DbPlanStatementKind::Select,
            DbPlanStatementKind::Returning,
        ] {
            let req = ExecuteRequest {
                operation_id: "op".into(),
                request_hash: String::new(),
                statements: vec![TypedDbStatement {
                    sql: "SELECT 1".into(),
                    parameters: vec![],
                    kind,
                    max_rows: 0,
                    result_selection: DbResultSelection::Rows,
                }],
                deadline_unix_ms: 0,
            };
            let bytes = encoded_execute_request_bytes(&req).unwrap();
            let back = decode_execute_request_bytes(&bytes).unwrap();
            assert_eq!(back.statements[0].kind, kind);
        }
        assert_eq!(
            serde_json::to_string(&DbPlanStatementKind::Returning).unwrap(),
            "\"returning\""
        );
    }
}
