//! Host→adapter execute envelope (canonical SQL + structured proofs).
//!
//! [`GuestDatabase::execute`] stays untrusted [`crate::ExecuteRequest`].
//! Adapters receive [`AdapterExecuteRequest`] with required 1:1 proofs.

use serde::{Deserialize, Serialize};

use crate::sql_proof::ResolvedStatement;
use crate::{ExecuteRequest, IsolationReq};

/// Host-stamped hint for adapters to persist guest replay payload before COMMIT.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GuestReceiptPersist {
    /// Guest statement count inside the receipt wrap (excluding prune/select).
    pub guest_statement_len: u32,
    /// Guest `requestHash` compared on replay.
    pub guest_request_hash: String,
}

#[cfg_attr(not(feature = "host"), allow(dead_code))]
impl GuestReceiptPersist {
    /// True when the host did not stamp a guest-receipt finalize hint.
    #[must_use]
    pub fn is_absent(&self) -> bool {
        self.guest_statement_len == 0
    }
}

/// Adapter execute payload: canonical SQL, structured proofs, optional receipt.
#[derive(Clone)]
pub struct AdapterExecuteRequest {
    /// Canonical execute payload (no proofs).
    pub request: ExecuteRequest,
    /// Host-stamped finalize hint for guest replay.
    pub guest_receipt: GuestReceiptPersist,
    /// One resolved proof per statement, bound to that statement's SQL.
    pub proofs: Vec<crate::sql_proof::ResolvedStatement>,
    /// Isolation the adapter must realize.
    pub isolation: IsolationReq,
}

#[cfg_attr(not(feature = "host"), allow(dead_code))]
impl AdapterExecuteRequest {
    /// Builds an adapter execute request without proofs.
    #[must_use]
    pub fn new(request: ExecuteRequest, guest_receipt: GuestReceiptPersist) -> Self {
        Self {
            request,
            guest_receipt,
            proofs: Vec::new(),
            isolation: IsolationReq::AtomicBatch,
        }
    }

    /// Stamps 1:1 proofs (including receipt wrappers).
    #[must_use]
    pub fn with_proofs(mut self, proofs: Vec<ResolvedStatement>) -> Self {
        self.proofs = proofs;
        self
    }

    /// Sets isolation (default [`IsolationReq::AtomicBatch`]).
    #[must_use]
    pub fn with_isolation(mut self, isolation: IsolationReq) -> Self {
        self.isolation = isolation;
        self
    }

    /// Fail closed unless proofs are 1:1 and hash-bound to each statement.
    ///
    /// # Errors
    ///
    /// Returns when the proof count differs or a proof is not bound to its SQL.
    pub fn require_proofs(&self) -> crate::Result<()> {
        if self.proofs.len() != self.request.statements.len() {
            return Err(crate::PluginError::internal(format!(
                "adapter execute proofs must match statement count ({} proofs, {} statements)",
                self.proofs.len(),
                self.request.statements.len()
            )));
        }
        for (stmt, proof) in self.request.statements.iter().zip(self.proofs.iter()) {
            if proof.statement_hash != crate::statement_sql_hash(stmt.sql.trim()) {
                return Err(crate::PluginError::internal(
                    "resolved SQL proof is not bound to this canonical statement",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;
    use crate::{DbPlanStatementKind, DbResultSelection, TypedDbStatement};

    fn select_req(sql: &str) -> ExecuteRequest {
        ExecuteRequest {
            operation_id: "op".into(),
            request_hash: String::new(),
            statements: vec![TypedDbStatement {
                sql: sql.into(),
                parameters: vec![],
                kind: DbPlanStatementKind::Select,
                max_rows: 1,
                result_selection: DbResultSelection::Rows,
            }],
            deadline_unix_ms: 0,
        }
    }

    #[test]
    fn require_proofs_rejects_missing_proofs() {
        let env =
            AdapterExecuteRequest::new(select_req("SELECT 1"), GuestReceiptPersist::default());
        assert!(env.require_proofs().is_err());
    }

    #[test]
    fn require_proofs_rejects_hash_mismatch() {
        let proof = crate::sql_proof::ResolvedStatement::bound_empty("SELECT 2");
        let err =
            AdapterExecuteRequest::new(select_req("SELECT 1"), GuestReceiptPersist::default())
                .with_proofs(vec![proof])
                .require_proofs()
                .expect_err("mismatched SQL must fail closed");
        assert!(err.to_string().contains("not bound"), "{err}");
    }

    #[test]
    fn require_proofs_accepts_hash_bound_proof() {
        let sql = "SELECT 1";
        let proof = crate::sql_proof::ResolvedStatement::bound_empty(sql);
        AdapterExecuteRequest::new(select_req(sql), GuestReceiptPersist::default())
            .with_proofs(vec![proof])
            .require_proofs()
            .expect("hash-bound proof");
    }
}
