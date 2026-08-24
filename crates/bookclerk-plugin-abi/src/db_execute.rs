//! Typed Cap'n database data-plane (`ExecuteRequest` / `ExecuteReply`) and
//! control-plane (`DbCapabilities`) mirrors of `plugin_v2.capnp`.
//!
//! First-party hosts call `DatabaseSession.capabilities` and
//! `DatabaseSession.executeAtomic`. JSON `bookclerk.capabilities` /
//! `bookclerk.atomic` sentinels remain for older `abiMinor` guests.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::db::{
    DbAtomicPlan, DbAtomicRequest, DbAtomicTiming, DbConnectResult, DbPlanExecResult,
    DbPlanStatement, DbPlanStatementKind, DbPlanStmtExecResult,
};
use crate::db_value::{db_value_from_json, db_value_to_json, DbType, DbValue};
use crate::v2::MAX_SCALAR_BYTES;

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

impl TypedDbStatement {
    /// Converts a JSON-bind plan statement onto the typed wire.
    ///
    /// # Errors
    ///
    /// Returns when a bind is outside the universal [`DbValue`] domain.
    pub fn from_plan_statement(stmt: &DbPlanStatement) -> Result<Self, String> {
        let parameters = stmt
            .binds
            .iter()
            .map(db_value_from_json)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            sql: stmt.sql.clone(),
            parameters,
            kind: stmt.kind,
            max_rows: stmt.max_rows,
            result_selection: selection_for_kind(stmt.kind),
        })
    }

    /// JSON-bind plan statement used by in-process executors.
    #[must_use]
    pub fn to_plan_statement(&self) -> DbPlanStatement {
        DbPlanStatement {
            sql: self.sql.clone(),
            binds: self.parameters.iter().map(db_value_to_json).collect(),
            kind: self.kind,
            max_rows: self.max_rows,
        }
    }
}

/// Default result selection from the host-authored kind.
fn selection_for_kind(kind: DbPlanStatementKind) -> DbResultSelection {
    match kind {
        DbPlanStatementKind::Execute => DbResultSelection::AffectedRows,
        _ => DbResultSelection::Rows,
    }
}

/// Typed atomic batch. Every request is a non-empty ordered statement list.
///
/// Host interpretation metadata (outcome / payload / receipt indices) lives on
/// [`DbAtomicPlan`] only, not on the wire request adapters execute.
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

impl ExecuteRequest {
    /// Typed request from a JSON [`DbAtomicRequest`].
    ///
    /// # Errors
    ///
    /// Returns when the plan is missing, empty, or a bind is not a [`DbValue`].
    pub fn from_atomic(req: &DbAtomicRequest) -> Result<Self, String> {
        let plan = req
            .plan
            .as_ref()
            .ok_or_else(|| "executeAtomic requires a host-authored plan".to_string())?;
        if plan.statements.is_empty() {
            return Err("executeAtomic statements must be non-empty".into());
        }
        let statements = plan
            .statements
            .iter()
            .map(TypedDbStatement::from_plan_statement)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            operation_id: req.operation_id.clone(),
            request_hash: req.request_hash.clone().unwrap_or_default(),
            statements,
            deadline_unix_ms: req.deadline_unix_ms.unwrap_or(0),
        })
    }

    /// JSON atomic envelope used by in-process executors.
    ///
    /// # Errors
    ///
    /// Returns when the statement list is empty.
    pub fn into_atomic(self) -> Result<DbAtomicRequest, String> {
        if self.statements.is_empty() {
            return Err("executeAtomic statements must be non-empty".into());
        }
        let plan = DbAtomicPlan {
            statements: self
                .statements
                .iter()
                .map(TypedDbStatement::to_plan_statement)
                .collect(),
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        Ok(DbAtomicRequest {
            operation_id: self.operation_id,
            request_hash: if self.request_hash.is_empty() {
                None
            } else {
                Some(self.request_hash)
            },
            plan: Some(plan),
            deadline_unix_ms: (self.deadline_unix_ms > 0).then_some(self.deadline_unix_ms),
        })
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

    /// Converts JSON plan rows onto typed columns + positional cells.
    ///
    /// # Errors
    ///
    /// Returns when a JSON cell is outside the universal domain, a row width
    /// does not match the column list, or column names are duplicated.
    pub fn from_plan_stmt(stmt: &DbPlanStmtExecResult) -> Result<Self, String> {
        let (columns, rows) = json_rows_to_typed(&stmt.rows)?;
        let result = Self {
            rows,
            columns,
            rows_affected: stmt.rows_affected,
        };
        result.validate_positional()?;
        Ok(result)
    }

    /// JSON object rows used by host `interpret_exec`.
    #[must_use]
    pub fn to_plan_stmt(&self) -> DbPlanStmtExecResult {
        DbPlanStmtExecResult {
            rows: typed_rows_to_json(&self.columns, &self.rows),
            rows_affected: self.rows_affected,
        }
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

impl From<DbAtomicTiming> for DbTiming {
    fn from(t: DbAtomicTiming) -> Self {
        Self {
            attempt_elapsed_us: t.attempt_elapsed_us,
            db_execution_us: t.db_execution_us.unwrap_or(0),
            db_timing_source: t.db_timing_source.unwrap_or_default(),
        }
    }
}

impl From<DbTiming> for DbAtomicTiming {
    fn from(t: DbTiming) -> Self {
        Self {
            attempt_elapsed_us: t.attempt_elapsed_us,
            db_execution_us: (t.db_execution_us > 0).then_some(t.db_execution_us),
            db_timing_source: (!t.db_timing_source.is_empty()).then_some(t.db_timing_source),
        }
    }
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

    /// Typed reply from a JSON [`DbPlanExecResult`].
    ///
    /// # Errors
    ///
    /// Returns when a JSON cell is outside the universal domain or a result
    /// row is not positional.
    pub fn from_plan_exec(result: &DbPlanExecResult) -> Result<Self, String> {
        let statements = result
            .statements
            .iter()
            .map(StatementResult::from_plan_stmt)
            .collect::<Result<Vec<_>, _>>()?;
        let reply = Self {
            operation_id: result.operation_id.clone(),
            statements,
            timing: result.timing.clone().map(Into::into).unwrap_or_default(),
        };
        reply.validate_positional()?;
        Ok(reply)
    }

    /// JSON plan result used by host interpretation.
    #[must_use]
    pub fn into_plan_exec(self) -> DbPlanExecResult {
        DbPlanExecResult {
            operation_id: self.operation_id,
            statements: self
                .statements
                .iter()
                .map(StatementResult::to_plan_stmt)
                .collect(),
            timing: Some(self.timing.into()),
        }
    }
}

/// Semantic SQL-contract advertisement (`DatabaseSession.capabilities`).
///
/// `diagnostic_engine` is observability only. Hosts must not branch on it for
/// plan compilation or schema selection.
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
    /// Observability-only engine name (`sqlite`, `postgres`, …).
    pub diagnostic_engine: String,
    /// Bootstrap-only SeaORM proxy hint (`sqlite` / `postgres`). Not used for
    /// host schema or plan selection; may be empty on conforming adapters.
    pub sql_family: String,
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
            diagnostic_engine: caps.dialect.clone(),
            sql_family: caps.sql_family.clone(),
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

    /// SeaORM proxy backend from bootstrap metadata only (not schema flags).
    ///
    /// # Errors
    ///
    /// Returns a message when bootstrap metadata does not name a supported engine.
    pub fn bootstrap_backend_failure_reason(&self) -> Option<String> {
        let connect = self.to_connect();
        connect.bootstrap_backend_failure_reason()
    }

    /// JSON connect result used by existing host negotiation.
    ///
    /// Schema flags and limits come from typed capabilities. [`Self::sql_family`]
    /// and [`Self::diagnostic_engine`] are passed through as bootstrap metadata
    /// for SeaORM proxy open only; they do not select host schema packs.
    #[must_use]
    pub fn to_connect(&self) -> DbConnectResult {
        let interactive_txn = !self.atomic_schema_batch;
        let sql_family = self.sql_family.clone();
        let dialect = if !self.sql_family.is_empty() {
            match self.sql_family.to_ascii_lowercase().as_str() {
                "postgres" | "postgresql" | "pg" => "postgres".into(),
                _ => "sqlite".into(),
            }
        } else if !self.diagnostic_engine.is_empty() {
            self.diagnostic_engine.clone()
        } else if self.pragma_user_version {
            "sqlite".into()
        } else {
            String::new()
        };
        DbConnectResult {
            dialect,
            interactive_txn,
            sql_family,
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

/// Converts JSON object rows onto typed columns + positional cells.
///
/// # Errors
///
/// Returns when a row is not a JSON object or a cell is outside [`DbValue`].
fn json_rows_to_typed(rows: &[serde_json::Value]) -> Result<(Vec<DbColumn>, Vec<DbRow>), String> {
    let Some(first) = rows.first() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let serde_json::Value::Object(map) = first else {
        return Err("atomic result row is not an object".into());
    };
    let columns: Vec<DbColumn> = map
        .keys()
        .map(|name| DbColumn {
            name: name.clone(),
            db_type: DbType::Unspecified,
        })
        .collect();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let serde_json::Value::Object(cells) = row else {
            return Err("atomic result row is not an object".into());
        };
        let mut values = Vec::with_capacity(columns.len());
        for col in &columns {
            let cell = cells.get(&col.name).unwrap_or(&serde_json::Value::Null);
            values.push(db_value_from_json(cell)?);
        }
        out.push(DbRow { values });
    }
    Ok((columns, out))
}

/// Converts typed rows back to JSON objects keyed by column name.
fn typed_rows_to_json(columns: &[DbColumn], rows: &[DbRow]) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            let mut map = serde_json::Map::new();
            for (i, value) in row.values.iter().enumerate() {
                let name = columns
                    .get(i)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| format!("_{i}"));
                map.insert(name, db_value_to_json(value));
            }
            serde_json::Value::Object(map)
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn atomic_request_roundtrip_preserves_binds() {
        let plan = DbAtomicPlan {
            statements: vec![DbPlanStatement::new(
                "SELECT ?",
                vec![json!(i64::MIN), json!("héllo\u{0}")],
                DbPlanStatementKind::Select,
            )],
            outcome_index: 0,
            payload_index: Some(0),
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let req = DbAtomicRequest::with_plan("op", "abc", plan);
        let typed = ExecuteRequest::from_atomic(&req).unwrap();
        assert_eq!(typed.statements[0].parameters[0], DbValue::Int64(i64::MIN));
        let back = typed.into_atomic().unwrap();
        assert_eq!(back.operation_id, "op");
        assert_eq!(back.request_hash.as_deref(), Some("abc"));
        assert_eq!(back.plan.unwrap().statements[0].binds[0], json!(i64::MIN));
    }

    #[test]
    fn empty_plan_is_rejected() {
        let req = DbAtomicRequest {
            operation_id: "op".into(),
            request_hash: None,
            plan: Some(DbAtomicPlan {
                statements: Vec::new(),
                outcome_index: 0,
                payload_index: None,
                prior_receipt_index: None,
                receipt_select_index: None,
            }),
            deadline_unix_ms: None,
        };
        assert!(ExecuteRequest::from_atomic(&req)
            .unwrap_err()
            .contains("non-empty"));
    }

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
            assert!(
                back.meets_host_minimums(),
                "{}",
                back.capability_failure_reason()
            );
        }
    }

    #[test]
    fn to_connect_ignores_diagnostic_engine() {
        let mut caps = DbCapabilities::from_connect(&DbConnectResult::sqlite());
        caps.diagnostic_engine = "postgres".into();
        let back = caps.to_connect();
        assert_eq!(back.dialect, "sqlite");
        assert_eq!(back.sql_family, "sqlite");
        assert!(back.interactive_txn);
        assert!(back.pragma_user_version);
    }

    #[test]
    fn to_connect_preserves_schema_flags_when_sql_family_differs() {
        let mut caps = DbCapabilities::from_connect(&DbConnectResult::d1());
        caps.sql_family = "postgres".into();
        let back = caps.to_connect();
        assert!(back.schema_migrations);
        assert!(back.atomic_schema_batch);
        assert!(!back.interactive_txn);
        assert_eq!(back.sql_family, "postgres");
        assert_eq!(back.dialect, "postgres");
    }

    #[test]
    fn capabilities_meet_minimums_without_sql_family() {
        let mut caps = DbCapabilities::from_connect(&DbConnectResult::sqlite());
        caps.sql_family.clear();
        assert!(
            caps.meets_host_minimums(),
            "{}",
            caps.capability_failure_reason()
        );
    }

    #[test]
    fn to_connect_does_not_infer_postgres_from_row_marker_flags() {
        let mut caps = DbCapabilities::from_connect(&DbConnectResult::d1());
        caps.sql_family.clear();
        let back = caps.to_connect();
        assert!(back.schema_migrations);
        assert!(back.atomic_schema_batch);
        assert!(back.sql_family.is_empty());
        assert_eq!(back.dialect, "sqlite");
    }

    #[test]
    fn bootstrap_rejects_unknown_sql_family() {
        let mut caps = DbCapabilities::from_connect(&DbConnectResult::sqlite());
        caps.sql_family = "mystery".into();
        let connect = caps.to_connect();
        let reason = connect.bootstrap_backend_failure_reason().expect("reject");
        assert!(reason.contains("sqlFamily"), "{reason}");
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
        assert_eq!(back, req);
        assert_eq!(
            hex::encode(&bytes),
            "000000001d00000000000000040003000000000000000000000000000000000000000000000000000000000000000000090000001a0000000900000022000000090000001f0000006f70000000000000616263000000000004000000010002000200020001000000050000004a000000090000004f00000053454c454354203f00000000000000000c00000002000100000002000000000000000000000000800000000000000000000004000000000000000000000000000d0000007200000000000500000000000000000000000000090000000a0000006236343a6e6f742d6279746573000000ff00000000000000"
        );
    }
}
