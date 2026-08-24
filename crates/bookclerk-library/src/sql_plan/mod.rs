//! Host-owned SQL atomic plans for database plugins.
//!
//! The library compiles Bookclerk domain operations into a generic
//! [`bookclerk_plugin_abi::DbAtomicPlan`]. Database guests execute the
//! statements as one transaction and return rows; they must not parse
//! domain operation names.

#[cfg(test)]
mod conformance;
mod exec;
mod guest_receipt;
mod interpret;
mod named;
mod reply;
mod slots;
mod typed_vectors;
pub mod vectors;

#[cfg(test)]
use bookclerk_plugin_abi::DbPlanStatementKind;
use bookclerk_plugin_abi::{
    encoded_execute_request_bytes, DbAtomicPlan, DbAtomicRequest, DbConnectResult, DbPlanStatement,
    ExecuteRequest,
};

pub use exec::{
    execute_plan_on, execute_plan_on_capped, execute_statements_on, execute_statements_on_session,
    AtomicSession,
};
pub(crate) use guest_receipt::{
    guest_receipt_persist_stmts, unwrap_guest_typed_reply, wrap_guest_typed_request,
};
pub use interpret::{interpret_exec, interpret_plan, validate_exec_result, PlanStmtResult};
pub use named::{compile_claim_event_delivery, compile_named_request};
pub use reply::validate_execute_reply;
pub use slots::{event_inflight_slot, lock_serialization_slot, JOB_QUEUE_SLOT};
pub use typed_vectors::run_typed_conn_vectors;

/// Compiled plan plus the hash stored on the receipt.
#[derive(Debug, Clone)]
pub struct CompiledAtomic {
    /// Wire plan executed by the guest.
    pub plan: DbAtomicPlan,
    /// SHA-256 hex compared on receipt replay.
    pub expected_hash: String,
}

impl CompiledAtomic {
    /// Envelope sent on `bookclerk.atomic`.
    #[must_use]
    pub fn into_request(self, operation_id: impl Into<String>) -> DbAtomicRequest {
        DbAtomicRequest::with_plan(operation_id, self.expected_hash, self.plan)
    }
}

/// Rejects a plan that exceeds negotiated guest limits or has out-of-range selectors.
///
/// # Errors
///
/// Returns [`crate::LibraryError::Other`] when the plan cannot be sent.
pub fn validate_plan(plan: &DbAtomicPlan, caps: &DbConnectResult) -> crate::error::Result<()> {
    let n_stmt = u32::try_from(plan.statements.len()).unwrap_or(u32::MAX);
    if caps.max_statements > 0 && n_stmt > caps.max_statements {
        return Err(crate::LibraryError::Other(anyhow::anyhow!(
            "atomic plan has {n_stmt} statements; guest maxStatements is {}",
            caps.max_statements
        )));
    }
    let stmt_len = plan.statements.len();
    let mut selectors = vec![("outcomeIndex", plan.outcome_index)];
    if let Some(idx) = plan.payload_index {
        selectors.push(("payloadIndex", idx));
    }
    if let Some(idx) = plan.prior_receipt_index {
        selectors.push(("priorReceiptIndex", idx));
    }
    if let Some(idx) = plan.receipt_select_index {
        selectors.push(("receiptSelectIndex", idx));
    }
    for (name, idx) in selectors {
        if (idx as usize) >= stmt_len {
            return Err(crate::LibraryError::Other(anyhow::anyhow!(
                "atomic plan {name} {idx} is out of range for {stmt_len} statements"
            )));
        }
    }
    for (i, stmt) in plan.statements.iter().enumerate() {
        let n_binds = u32::try_from(stmt.binds.len()).unwrap_or(u32::MAX);
        if caps.max_binds > 0 && n_binds > caps.max_binds {
            return Err(crate::LibraryError::Other(anyhow::anyhow!(
                "atomic plan statement {i} has {n_binds} binds; guest maxBinds is {}",
                caps.max_binds
            )));
        }
        if caps.max_payload_bytes > 0 {
            let binds_len = serde_json::to_vec(&stmt.binds)
                .map(|b| b.len())
                .unwrap_or(0);
            let payload = stmt.sql.len().saturating_add(binds_len);
            let cap = usize::try_from(caps.max_payload_bytes).unwrap_or(usize::MAX);
            if payload > cap {
                return Err(crate::LibraryError::Other(anyhow::anyhow!(
                    "atomic plan statement {i} encoded payload is {payload} bytes; guest maxPayloadBytes is {}",
                    caps.max_payload_bytes
                )));
            }
        }
    }
    Ok(())
}

/// Rejects a typed [`ExecuteRequest`] that exceeds negotiated guest limits.
///
/// `maxAtomicRequestBytes` is measured against the Cap'n-encoded
/// [`ExecuteRequest`] that will be sent, not the JSON [`DbAtomicRequest`]
/// envelope used by older-minor sentinels.
///
/// # Errors
///
/// Returns [`crate::LibraryError::Other`] when the request cannot be sent.
pub fn validate_execute_request(
    req: &ExecuteRequest,
    caps: &DbConnectResult,
) -> crate::error::Result<()> {
    if req.statements.is_empty() {
        return Err(crate::LibraryError::Other(anyhow::anyhow!(
            "executeAtomic statements must be non-empty"
        )));
    }
    let n_stmt = u32::try_from(req.statements.len()).unwrap_or(u32::MAX);
    if caps.max_statements > 0 && n_stmt > caps.max_statements {
        return Err(crate::LibraryError::Other(anyhow::anyhow!(
            "atomic request has {n_stmt} statements; guest maxStatements is {}",
            caps.max_statements
        )));
    }
    for (i, stmt) in req.statements.iter().enumerate() {
        let n_binds = u32::try_from(stmt.parameters.len()).unwrap_or(u32::MAX);
        if caps.max_binds > 0 && n_binds > caps.max_binds {
            return Err(crate::LibraryError::Other(anyhow::anyhow!(
                "atomic request statement {i} has {n_binds} binds; guest maxBinds is {}",
                caps.max_binds
            )));
        }
        if caps.max_payload_bytes > 0 {
            let mut binds_len = 0usize;
            for param in &stmt.parameters {
                binds_len = binds_len.saturating_add(
                    bookclerk_plugin_abi::encoded_db_value_bytes(param)
                        .map(|b| b.len())
                        .unwrap_or(usize::MAX),
                );
            }
            let payload = stmt.sql.len().saturating_add(binds_len);
            let cap = usize::try_from(caps.max_payload_bytes).unwrap_or(usize::MAX);
            if payload > cap {
                return Err(crate::LibraryError::Other(anyhow::anyhow!(
                    "atomic request statement {i} encoded payload is {payload} bytes; guest maxPayloadBytes is {}",
                    caps.max_payload_bytes
                )));
            }
        }
    }
    let cap = atomic_request_cap_bytes(caps);
    if cap == 0 {
        return Ok(());
    }
    let bytes = encoded_execute_request_bytes(req)
        .map(|b| b.len())
        .unwrap_or(usize::MAX);
    if bytes > cap {
        return Err(crate::LibraryError::Other(anyhow::anyhow!(
            "atomic request is {bytes} bytes; guest maxAtomicRequestBytes is {cap}"
        )));
    }
    Ok(())
}

/// Host-authorizes a guest typed batch: overwrite statement kinds, enforce
/// negotiated caps, and stamp the canonical Cap'n request hash.
///
/// A non-empty guest `requestHash` must match the host digest (retry token).
/// An empty `operationId` is replaced with a fresh UUID.
///
/// # Errors
///
/// Returns [`crate::LibraryError::Other`] when the request exceeds caps, the
/// retry hash does not match, or the canonical digest cannot be encoded.
pub fn authorize_typed_request(
    req: &mut ExecuteRequest,
    caps: &DbConnectResult,
) -> crate::error::Result<()> {
    for stmt in &mut req.statements {
        stmt.kind = host_statement_kind(&stmt.sql);
    }
    if req.operation_id.is_empty() {
        req.operation_id = uuid::Uuid::new_v4().to_string();
    }
    let computed = bookclerk_plugin_abi::canonical_execute_request_hash(req)
        .map_err(|err| crate::LibraryError::Other(anyhow::anyhow!(err.to_string())))?;
    if !req.request_hash.is_empty() && req.request_hash != computed {
        return Err(crate::LibraryError::Other(anyhow::anyhow!(
            "retry token requestHash does not match the canonical request"
        )));
    }
    req.request_hash = computed;
    validate_execute_request(req, caps)?;
    Ok(())
}

/// Authorizes a **guest-authored** typed batch: grammar and table scope,
/// bind counts, result selection, then [`authorize_typed_request`].
///
/// Host schema DDL must use [`authorize_typed_request`] directly.
///
/// # Errors
///
/// Returns [`crate::LibraryError::Other`] when the SQL is outside the guest
/// grammar or fails host validation.
pub fn authorize_guest_typed_request(
    req: &mut ExecuteRequest,
    caps: &DbConnectResult,
    policy: &bookclerk_plugin_abi::GuestSqlPolicy,
) -> crate::error::Result<()> {
    bookclerk_plugin_abi::validate_guest_execute_request(req)
        .map_err(|err| crate::LibraryError::Other(anyhow::anyhow!(err.to_string())))?;
    bookclerk_plugin_abi::authorize_guest_sql_policy(req, policy)
        .map_err(|err| crate::LibraryError::Other(anyhow::anyhow!(err.to_string())))?;
    authorize_typed_request(req, caps)
}

/// Rejects a plan or encoded [`DbAtomicRequest`] that exceeds negotiated guest limits.
///
/// # Errors
///
/// Returns [`crate::LibraryError::Other`] when the request cannot be sent.
pub fn validate_atomic_request(
    req: &DbAtomicRequest,
    caps: &DbConnectResult,
) -> crate::error::Result<()> {
    if let Some(plan) = &req.plan {
        validate_plan(plan, caps)?;
    } else {
        return Err(crate::LibraryError::Other(anyhow::anyhow!(
            "dbAtomic requires a host-authored executePlan"
        )));
    }
    let cap = atomic_request_cap_bytes(caps);
    if cap == 0 {
        return Ok(());
    }
    let bytes = serde_json::to_vec(req)
        .map(|b| b.len())
        .unwrap_or(usize::MAX);
    if bytes > cap {
        return Err(crate::LibraryError::Other(anyhow::anyhow!(
            "atomic request is {bytes} bytes; guest maxAtomicRequestBytes is {cap}"
        )));
    }
    Ok(())
}

/// JSON-byte budget for one encoded [`DbAtomicRequest`] (`0` = skip the check).
fn atomic_request_cap_bytes(caps: &DbConnectResult) -> usize {
    if caps.max_atomic_request_bytes == 0 {
        return 0;
    }
    usize::try_from(
        caps.max_atomic_request_bytes
            .min(bookclerk_plugin_abi::v2::MAX_SCALAR_BYTES),
    )
    .unwrap_or(usize::MAX)
}

/// Wake page size from negotiated `maxBinds` (SET/EXISTS overhead is 4 binds).
#[must_use]
pub fn wake_page_for_max_binds(max_binds: u32) -> u64 {
    const FIXED: u32 = 4;
    u64::from(max_binds.saturating_sub(FIXED).clamp(8, 256))
}

/// Converts an internal statement list into the wire plan.
fn wire_plan(
    statements: Vec<named::SqlStmt>,
    outcome_index: usize,
    payload_index: Option<usize>,
    prior_receipt_index: Option<usize>,
    receipt_select_index: Option<usize>,
) -> DbAtomicPlan {
    DbAtomicPlan {
        statements: statements
            .into_iter()
            .map(|stmt| DbPlanStatement {
                sql: stmt.sql,
                binds: stmt.binds,
                kind: stmt.kind,
                max_rows: stmt.max_rows,
            })
            .collect(),
        outcome_index: u32::try_from(outcome_index).unwrap_or(0),
        payload_index: payload_index.and_then(|i| u32::try_from(i).ok()),
        prior_receipt_index: prior_receipt_index.and_then(|i| u32::try_from(i).ok()),
        receipt_select_index: receipt_select_index.and_then(|i| u32::try_from(i).ok()),
    }
}

/// Host-authored statement shape for the wire `kind` field.
///
/// Adapters must not reparse SQL; they trust this classification.
#[must_use]
pub fn host_statement_kind(sql: &str) -> bookclerk_plugin_abi::DbPlanStatementKind {
    named::authored_kind(sql)
}

/// Kind for a SeaORM proxy **read**. `SELECT`/`WITH`/`VALUES` become `Select`
/// (LIMIT-wrapped). `RETURNING` stays `Returning`. Everything else (PRAGMA,
/// schema introspection) is `Query` so it is not LIMIT-wrapped.
#[must_use]
pub fn proxy_read_kind(sql: &str) -> bookclerk_plugin_abi::DbPlanStatementKind {
    match named::authored_kind(sql) {
        bookclerk_plugin_abi::DbPlanStatementKind::Select => {
            bookclerk_plugin_abi::DbPlanStatementKind::Select
        }
        bookclerk_plugin_abi::DbPlanStatementKind::Returning => {
            bookclerk_plugin_abi::DbPlanStatementKind::Returning
        }
        _ => bookclerk_plugin_abi::DbPlanStatementKind::Query,
    }
}

/// Kind for a SeaORM proxy **write**. `RETURNING` is preserved; other DML is
/// `Execute`.
#[must_use]
pub fn proxy_write_kind(sql: &str) -> bookclerk_plugin_abi::DbPlanStatementKind {
    match named::authored_kind(sql) {
        bookclerk_plugin_abi::DbPlanStatementKind::Returning => {
            bookclerk_plugin_abi::DbPlanStatementKind::Returning
        }
        _ => bookclerk_plugin_abi::DbPlanStatementKind::Execute,
    }
}

#[cfg(test)]
mod limits_tests {
    use super::{
        validate_atomic_request, validate_execute_request, validate_plan, DbAtomicPlan,
        DbAtomicRequest, DbConnectResult, DbPlanStatement, DbPlanStatementKind,
    };

    fn tiny_caps() -> DbConnectResult {
        let mut caps = DbConnectResult::sqlite();
        caps.max_statements = 2;
        caps.max_binds = 2;
        caps.max_payload_bytes = 64;
        caps
    }

    fn stmt(sql: &str, binds: Vec<serde_json::Value>) -> DbPlanStatement {
        DbPlanStatement::new(sql, binds, DbPlanStatementKind::Query)
    }

    #[test]
    fn validate_plan_rejects_too_many_statements() {
        let plan = DbAtomicPlan {
            statements: vec![
                stmt("SELECT 1", vec![]),
                stmt("SELECT 2", vec![]),
                stmt("SELECT 3", vec![]),
            ],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let err = validate_plan(&plan, &tiny_caps()).unwrap_err();
        assert!(err.to_string().contains("maxStatements"), "{err}");
    }

    #[test]
    fn validate_plan_rejects_too_many_binds() {
        let plan = DbAtomicPlan {
            statements: vec![stmt(
                "SELECT ?, ?, ?",
                vec![
                    serde_json::json!(1),
                    serde_json::json!(2),
                    serde_json::json!(3),
                ],
            )],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let err = validate_plan(&plan, &tiny_caps()).unwrap_err();
        assert!(err.to_string().contains("maxBinds"), "{err}");
    }

    #[test]
    fn validate_plan_rejects_payload_bytes() {
        let plan = DbAtomicPlan {
            statements: vec![stmt("SELECT ?", vec![serde_json::json!("x".repeat(200))])],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let err = validate_plan(&plan, &tiny_caps()).unwrap_err();
        assert!(err.to_string().contains("maxPayloadBytes"), "{err}");
    }

    #[test]
    fn validate_plan_rejects_oversized_dispatch_page() {
        use crate::atomic_ops::DbAtomicParams;
        use crate::sql_plan::compile_named_request;
        let subs: Vec<serde_json::Value> = (0..50)
            .map(|i| serde_json::json!({ "pluginId": format!("p{i}") }))
            .collect();
        let compiled = compile_named_request(
            "oversize",
            &DbAtomicParams::DispatchEventDeliveries {
                event_id: "evt".into(),
                subscribers_json: serde_json::to_string(&subs).unwrap(),
                mark_dispatched: true,
            },
            "2024-06-01T00:00:00Z",
        )
        .unwrap();
        let mut caps = DbConnectResult::sqlite();
        caps.max_statements = 40;
        let err = validate_plan(&compiled.plan, &caps).unwrap_err();
        assert!(err.to_string().contains("maxStatements"), "{err}");
        assert!(
            compiled.plan.statements.len() > 40,
            "planner must emit inserts for every subscriber, not take(24)"
        );
    }

    #[test]
    fn validate_atomic_request_rejects_oversized_envelope() {
        let plan = DbAtomicPlan {
            statements: vec![stmt(&format!("SELECT '{}'", "x".repeat(5000)), vec![])],
            outcome_index: 0,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let mut caps = DbConnectResult::sqlite();
        caps.max_payload_bytes = 1_048_576;
        caps.max_atomic_request_bytes = 4_096;
        caps.max_atomic_result_bytes = 4_096;
        caps.max_result_bytes = 4_096;
        let req = DbAtomicRequest::with_plan("op", "abc", plan);
        let err = validate_atomic_request(&req, &caps).unwrap_err();
        assert!(err.to_string().contains("maxAtomicRequestBytes"), "{err}");
        let typed = bookclerk_plugin_abi::ExecuteRequest::from_atomic(&req).unwrap();
        let err = validate_execute_request(&typed, &caps).unwrap_err();
        assert!(err.to_string().contains("maxAtomicRequestBytes"), "{err}");
        let mut typed = typed;
        typed.request_hash.clear();
        let err = super::authorize_typed_request(&mut typed, &caps).unwrap_err();
        assert!(err.to_string().contains("maxAtomicRequestBytes"), "{err}");
    }

    #[test]
    fn authorize_typed_request_rejects_over_max_binds_and_stamps_hash() {
        use bookclerk_plugin_abi::{
            canonical_execute_request_hash, DbResultSelection, DbValue, ExecuteRequest,
            TypedDbStatement,
        };
        let mut caps = DbConnectResult::sqlite();
        caps.max_binds = 1;
        let mut req = ExecuteRequest {
            operation_id: "guest-op".into(),
            request_hash: String::new(),
            statements: vec![TypedDbStatement {
                sql: "SELECT ?, ?".into(),
                parameters: vec![DbValue::Int64(1), DbValue::Int64(2)],
                kind: DbPlanStatementKind::Execute,
                max_rows: 0,
                result_selection: DbResultSelection::Rows,
            }],
            deadline_unix_ms: 0,
        };
        let err = super::authorize_typed_request(&mut req, &caps).unwrap_err();
        assert!(err.to_string().contains("maxBinds"), "{err}");

        caps.max_binds = 32;
        req.request_hash.clear();
        super::authorize_typed_request(&mut req, &caps).unwrap();
        assert_eq!(req.statements[0].kind, DbPlanStatementKind::Select);
        let expected = canonical_execute_request_hash(&req).unwrap();
        assert_eq!(req.request_hash, expected);
        req.request_hash = "deadbeef".into();
        let err = super::authorize_typed_request(&mut req, &caps).unwrap_err();
        assert!(err.to_string().contains("requestHash"), "{err}");
    }

    #[test]
    fn authorize_typed_request_measures_size_after_stamping_hash() {
        use bookclerk_plugin_abi::{
            encoded_execute_request_bytes, DbResultSelection, ExecuteRequest, TypedDbStatement,
        };
        let mut req = ExecuteRequest {
            operation_id: "guest-op".into(),
            request_hash: String::new(),
            statements: vec![TypedDbStatement {
                sql: "SELECT 1".into(),
                parameters: vec![],
                kind: DbPlanStatementKind::Select,
                max_rows: 1,
                result_selection: DbResultSelection::Rows,
            }],
            deadline_unix_ms: 0,
        };
        let empty_len = encoded_execute_request_bytes(&req).unwrap().len();
        let mut stamped = req.clone();
        stamped.request_hash = "a".repeat(64);
        let stamped_len = encoded_execute_request_bytes(&stamped).unwrap().len();
        assert!(
            stamped_len > empty_len,
            "stamped hash must grow the Cap'n encoding ({stamped_len} vs {empty_len})"
        );
        let mut caps = DbConnectResult::sqlite();
        caps.max_payload_bytes = 1_048_576;
        caps.max_atomic_request_bytes = u32::try_from(empty_len + 1).unwrap();
        caps.max_atomic_result_bytes = 1_048_576;
        caps.max_result_bytes = 1_048_576;
        let err = super::authorize_typed_request(&mut req, &caps).unwrap_err();
        assert!(err.to_string().contains("maxAtomicRequestBytes"), "{err}");
    }

    #[test]
    fn canonical_hash_ignores_deadline_unix_ms() {
        use bookclerk_plugin_abi::{
            canonical_execute_request_hash, DbResultSelection, ExecuteRequest, TypedDbStatement,
        };
        let mut req = ExecuteRequest {
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
        let a = canonical_execute_request_hash(&req).unwrap();
        req.deadline_unix_ms = 1;
        let b = canonical_execute_request_hash(&req).unwrap();
        req.deadline_unix_ms = 9_999_999_999;
        let c = canonical_execute_request_hash(&req).unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
        req.operation_id = "other".into();
        req.request_hash = "ffff".into();
        assert_eq!(a, canonical_execute_request_hash(&req).unwrap());
    }

    #[test]
    fn validate_plan_rejects_out_of_range_selector() {
        let plan = DbAtomicPlan {
            statements: vec![stmt("SELECT 1", vec![])],
            outcome_index: 3,
            payload_index: None,
            prior_receipt_index: None,
            receipt_select_index: None,
        };
        let err = validate_plan(&plan, &DbConnectResult::sqlite()).unwrap_err();
        assert!(err.to_string().contains("outcomeIndex"), "{err}");
    }

    #[test]
    fn authorize_guest_typed_request_rejects_ddl_tables_binds_and_selection() {
        use bookclerk_plugin_abi::{DbResultSelection, DbValue, ExecuteRequest, TypedDbStatement};
        let caps = DbConnectResult::sqlite();
        let books = bookclerk_plugin_abi::GuestSqlPolicy::allow_tables(["books"]);
        let mut ddl = ExecuteRequest {
            operation_id: "g".into(),
            request_hash: String::new(),
            statements: vec![TypedDbStatement {
                sql: "DROP TABLE books".into(),
                parameters: vec![],
                kind: DbPlanStatementKind::Execute,
                max_rows: 0,
                result_selection: DbResultSelection::AffectedRows,
            }],
            deadline_unix_ms: 0,
        };
        let err = super::authorize_guest_typed_request(&mut ddl, &caps, &books).unwrap_err();
        assert!(err.to_string().contains("disallowed"), "{err}");

        let mut secrets = ddl.clone();
        secrets.statements[0].sql = "SELECT token FROM encrypted_secrets".into();
        secrets.statements[0].result_selection = DbResultSelection::Rows;
        let err = super::authorize_guest_typed_request(&mut secrets, &caps, &books).unwrap_err();
        assert!(err.to_string().contains("unauthorized"), "{err}");

        let mut binds = ddl.clone();
        binds.statements[0].sql = "SELECT ? FROM books WHERE id = ?".into();
        binds.statements[0].parameters = vec![DbValue::Int64(1)];
        binds.statements[0].result_selection = DbResultSelection::Rows;
        let err = super::authorize_guest_typed_request(&mut binds, &caps, &books).unwrap_err();
        assert!(err.to_string().contains("placeholder"), "{err}");

        let mut rows_on_insert = ddl.clone();
        rows_on_insert.statements[0].sql = "INSERT INTO books (id) VALUES (?)".into();
        rows_on_insert.statements[0].parameters = vec![DbValue::Int64(1)];
        rows_on_insert.statements[0].result_selection = DbResultSelection::Rows;
        rows_on_insert.statements[0].max_rows = 1;
        let err =
            super::authorize_guest_typed_request(&mut rows_on_insert, &caps, &books).unwrap_err();
        assert!(err.to_string().contains("row-producing"), "{err}");

        let mut select = ddl.clone();
        select.statements[0].sql = "SELECT id FROM books".into();
        select.statements[0].kind = DbPlanStatementKind::Query;
        select.statements[0].result_selection = DbResultSelection::Rows;
        super::authorize_guest_typed_request(&mut select, &caps, &books).unwrap();
        assert_eq!(select.statements[0].kind, DbPlanStatementKind::Select);
        assert!(!select.request_hash.is_empty());

        let mut jobs = select.clone();
        jobs.statements[0].sql = "SELECT id FROM jobs".into();
        let err = super::authorize_guest_typed_request(&mut jobs, &caps, &books).unwrap_err();
        assert!(err.to_string().contains("unauthorized table"), "{err}");
    }

    #[test]
    fn canonical_hash_deadline_golden_matches_sdks() {
        use bookclerk_plugin_abi::{
            canonical_execute_request_hash, DbResultSelection, ExecuteRequest, TypedDbStatement,
        };
        let mut req = ExecuteRequest {
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
        const GOLDEN: &str = "e368ef90b76963c5e93c5e6db37fdb6d7f809d23c10295352a0ba3cd26885f02";
        assert_eq!(canonical_execute_request_hash(&req).unwrap(), GOLDEN);
        req.deadline_unix_ms = 9_999_999_999;
        assert_eq!(canonical_execute_request_hash(&req).unwrap(), GOLDEN);
    }

    #[test]
    fn host_kind_marks_cte_insert_returning_not_select() {
        let sql = "WITH seed AS (SELECT 1) INSERT INTO t SELECT * FROM seed RETURNING id";
        assert_eq!(
            super::host_statement_kind(sql),
            DbPlanStatementKind::Returning
        );
        assert!(!DbPlanStatementKind::Returning.wrap_select_limit());
        assert_eq!(
            super::host_statement_kind("SELECT x FROM t"),
            DbPlanStatementKind::Select
        );
        assert_eq!(
            super::host_statement_kind(
                "WITH RECURSIVE t(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM t WHERE x < 3) SELECT x FROM t"
            ),
            DbPlanStatementKind::Select
        );
        assert_eq!(
            super::host_statement_kind("WITH seed AS (SELECT 1) INSERT INTO t SELECT * FROM seed"),
            DbPlanStatementKind::Execute
        );
        assert_eq!(
            super::host_statement_kind("WITH seed AS (SELECT 1) UPDATE t SET x = 1"),
            DbPlanStatementKind::Execute
        );
        assert_eq!(
            super::host_statement_kind("WITH seed AS (SELECT 1) DELETE FROM t"),
            DbPlanStatementKind::Execute
        );
        assert_eq!(
            super::host_statement_kind("WITH seed AS (SELECT 1) UPDATE t SET x = 1 RETURNING x"),
            DbPlanStatementKind::Returning
        );
        assert_eq!(
            super::host_statement_kind("WITH seed AS (SELECT 1) DELETE FROM t RETURNING id"),
            DbPlanStatementKind::Returning
        );
    }
}
