//! Typed Cap'n database data-plane (`ExecuteRequest` / `ExecuteReply`) and
//! control-plane (`DbCapabilities`) mirrors of `plugin_v2.capnp`.
//!
//! First-party hosts call `DatabaseSession.capabilities` and
//! `DatabaseSession.executeAtomic`. JSON `bookclerk.capabilities` /
//! `bookclerk.atomic` sentinels remain for older `abiMinor` guests.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::db::DbConnectResult;
use crate::db_value::{DbType, DbValue};
use crate::v2::MAX_SCALAR_BYTES;

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
    ///
    /// Legacy wire/JSON `"query"` deserializes as this variant.
    #[default]
    #[serde(alias = "query")]
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

/// Host-only hint for adapters to persist guest replay payload before COMMIT.
///
/// Plugin authors must not set this field. The host stamps it when wrapping
/// guest `executeAtomic` batches with a durable receipt envelope.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GuestReceiptPersist {
    /// Guest statement count inside the receipt wrap (excluding prune/select).
    pub guest_statement_len: u32,
    /// Guest `requestHash` compared on replay.
    pub guest_request_hash: String,
}

impl GuestReceiptPersist {
    /// True when the host did not stamp a guest-receipt finalize hint.
    #[must_use]
    pub fn is_absent(&self) -> bool {
        self.guest_statement_len == 0
    }
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

/// Host-only guest receipt hint (not on the public Cap'n `ExecuteRequest`).
#[derive(Clone)]
pub struct HostExecuteEnvelope {
    /// Public execute payload.
    pub request: ExecuteRequest,
    /// Host-only finalize hint stamped by guest receipt wrap.
    pub guest_receipt: GuestReceiptPersist,
}

impl HostExecuteEnvelope {
    /// Builds a host-private envelope for adapter execution.
    #[must_use]
    pub fn new(request: ExecuteRequest, guest_receipt: GuestReceiptPersist) -> Self {
        Self {
            request,
            guest_receipt,
        }
    }
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
/// Bootstrap metadata (`sql_family`, `diagnostic_engine`, SeaORM `dialect`) lives
/// on JSON [`DbConnectResult`] only — not on this typed capability plane.
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
}

impl DbCapabilities {
    /// Typed advertisement from a JSON connect result.
    #[must_use]
    pub fn from_connect(caps: &DbConnectResult) -> Self {
        Self {
            sql_contract_version: caps.sql_contract_version,
            atomic_batch: caps.atomic_batch,
            returning: caps.returning,
            affected_rows: true,
            schema_migrations: caps.schema_migrations,
            pragma_user_version: caps.pragma_user_version,
            atomic_schema_batch: caps.atomic_schema_batch,
            cancellation: true,
            timing: caps.timing,
            max_binds: caps.max_binds,
            max_statements: caps.max_statements,
            max_result_rows: caps.max_result_rows,
            max_payload_bytes: caps.max_payload_bytes,
            max_result_bytes: caps.max_result_bytes,
            max_cell_bytes: caps.max_cell_bytes,
            max_request_bytes: caps.max_atomic_request_bytes.max(caps.max_payload_bytes),
            max_atomic_result_bytes: caps.max_atomic_result_bytes,
        }
    }

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
        let connect = self.to_connect();
        if !connect.meets_host_minimums() {
            return Some(connect.capability_failure_reason());
        }
        None
    }

    /// JSON connect result with semantic flags and limits only.
    ///
    /// Bootstrap metadata (`dialect`, `sql_family`) is left empty; the host
    /// connect path merges it from JSON `dbConnect` / plugin bootstrap rules.
    #[must_use]
    pub fn to_connect(&self) -> DbConnectResult {
        DbConnectResult {
            dialect: String::new(),
            interactive_txn: !self.atomic_schema_batch,
            sql_family: String::new(),
            atomic_batch: self.atomic_batch,
            returning: self.returning,
            max_binds: self.max_binds,
            max_statements: self.max_statements,
            max_result_rows: self.max_result_rows,
            max_payload_bytes: self.max_payload_bytes,
            max_result_bytes: self.max_result_bytes,
            max_cell_bytes: self.max_cell_bytes,
            max_atomic_request_bytes: self.max_request_bytes.min(MAX_SCALAR_BYTES),
            max_atomic_result_bytes: self.max_atomic_result_bytes,
            sql_contract_version: self.sql_contract_version,
            pragma_user_version: self.pragma_user_version,
            schema_migrations: self.schema_migrations,
            atomic_schema_batch: self.atomic_schema_batch,
            timing: self.timing,
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
        let mut caps = DbCapabilities::from_connect(&DbConnectResult::sqlite());
        caps.cancellation = false;
        assert!(!caps.meets_host_minimums());
        assert!(caps.capability_failure_reason().contains("cancellation"));
    }

    #[test]
    fn capabilities_reject_missing_affected_rows() {
        let mut caps = DbCapabilities::from_connect(&DbConnectResult::sqlite());
        caps.affected_rows = false;
        assert!(!caps.meets_host_minimums());
        assert!(caps.capability_failure_reason().contains("affectedRows"));
    }

    #[test]
    fn capabilities_roundtrip_schema_flags() {
        for caps in [
            DbConnectResult::sqlite(),
            DbConnectResult::postgres(),
            DbConnectResult::d1(),
        ] {
            let typed = DbCapabilities::from_connect(&caps);
            let back = typed.to_connect();
            assert_eq!(back.pragma_user_version, caps.pragma_user_version);
            assert_eq!(back.schema_migrations, caps.schema_migrations);
            assert_eq!(back.atomic_schema_batch, caps.atomic_schema_batch);
            assert_eq!(back.interactive_txn, caps.interactive_txn);
            assert!(back.sql_family.is_empty());
            assert!(back.dialect.is_empty());
            assert!(
                back.meets_host_minimums(),
                "{}",
                back.capability_failure_reason()
            );
        }
    }

    #[test]
    fn capabilities_meet_minimums_without_bootstrap_metadata() {
        let caps = DbCapabilities::from_connect(&DbConnectResult::sqlite());
        assert!(
            caps.meets_host_minimums(),
            "{}",
            caps.capability_failure_reason()
        );
    }

    #[test]
    fn to_connect_leaves_bootstrap_empty() {
        let caps = DbCapabilities::from_connect(&DbConnectResult::d1());
        let back = caps.to_connect();
        assert!(back.schema_migrations);
        assert!(back.atomic_schema_batch);
        assert!(!back.interactive_txn);
        assert!(back.sql_family.is_empty());
        assert!(back.dialect.is_empty());
    }

    #[test]
    fn bootstrap_failure_checked_on_connect_result_not_capabilities() {
        let mut connect = DbConnectResult::sqlite();
        connect.sql_family = "mystery".into();
        let reason = connect.bootstrap_backend_failure_reason().expect("reject");
        assert!(reason.contains("sqlFamily"), "{reason}");
        let typed = DbCapabilities::from_connect(&DbConnectResult::sqlite());
        assert!(typed.meets_host_minimums());
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
        // Legacy JSON `"query"` deserializes as Returning (host-compat only).
        let kind: DbPlanStatementKind = serde_json::from_str("\"query\"").unwrap();
        assert_eq!(kind, DbPlanStatementKind::Returning);
        assert_eq!(
            serde_json::to_string(&DbPlanStatementKind::Returning).unwrap(),
            "\"returning\""
        );
    }
}
