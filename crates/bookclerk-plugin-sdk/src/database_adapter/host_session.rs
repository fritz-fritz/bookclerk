//! Host-private adapter session helpers (first-party database guests only).

#![allow(clippy::missing_docs_in_private_items)]

use bookclerk_plugin_abi::host_envelope::HostExecuteEnvelope;
use bookclerk_plugin_abi::v2::{AdapterTransaction, HostAdapterDatabaseSession};
use bookclerk_plugin_abi::{ExecuteReply, ExecuteRequest, Result};

use super::errors::plugin_error_from_engine;
use super::session::{
    guest_begin, guest_commit, guest_execute_atomic, guest_execute_atomic_on_txn, guest_rollback,
};

/// In-process SeaORM transaction bridge for host `begin` RPC.
pub struct GuestHostAdapterSession;

#[async_trait::async_trait(?Send)]
impl HostAdapterDatabaseSession for GuestHostAdapterSession {
    async fn begin(&self) -> Result<Box<dyn AdapterTransaction>> {
        let txn_id = guest_begin(None).await.map_err(plugin_error_from_engine)?;
        Ok(Box::new(GuestHostAdapterTransaction { txn_id }))
    }

    async fn execute_envelope(&self, envelope: HostExecuteEnvelope) -> Result<ExecuteReply> {
        guest_execute_atomic(envelope)
            .await
            .map_err(plugin_error_from_engine)
    }
}

struct GuestHostAdapterTransaction {
    txn_id: String,
}

#[async_trait::async_trait(?Send)]
impl AdapterTransaction for GuestHostAdapterTransaction {
    async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteReply> {
        guest_execute_atomic_on_txn(self.txn_id.clone(), request).await
    }

    async fn commit(&self) -> Result<()> {
        guest_commit(self.txn_id.clone())
            .await
            .map_err(plugin_error_from_engine)
    }

    async fn rollback(&self) -> Result<()> {
        guest_rollback(self.txn_id.clone())
            .await
            .map_err(plugin_error_from_engine)
    }
}

/// Maps engine failures from [`guest_begin`] to structured plugin errors.
pub fn host_session() -> GuestHostAdapterSession {
    GuestHostAdapterSession
}
