//! Host-private JSON database transport DTOs (interactive txn proxy).
//!
//! Not part of the public plugin author contract. First-party hosts use these
//! with the legacy SeaORM stdio/JSON proxy alongside typed `executeAtomic`.

use serde::{Deserialize, Serialize};

/// Params for host-internal [`crate::methods::db_begin`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbBeginParams {
    /// Existing transaction to nest a savepoint under (wire `parentTxnId`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_txn_id: Option<String>,
}

/// Result of a successful host-internal [`crate::methods::db_begin`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbBeginResult {
    /// Opaque id the host must send on subsequent statements and commit/rollback.
    pub txn_id: String,
}

/// Params for host-internal [`crate::methods::db_commit`] / [`crate::methods::db_rollback`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbTxnParams {
    /// Transaction id returned by [`crate::methods::db_begin`].
    pub txn_id: String,
}
