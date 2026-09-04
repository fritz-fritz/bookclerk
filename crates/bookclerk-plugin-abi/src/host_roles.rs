//! Host-private ABI roles (not exposed to third-party plugin authors).

use crate::Result;

/// Host-only interactive transaction on an adapter connection (SeaORM proxy).
#[async_trait::async_trait(?Send)]
pub trait AdapterTransaction {
    /// Typed statements on this open transaction (no second `BEGIN`).
    ///
    /// Proofs are required ([`crate::AdapterExecuteRequest::require_proofs`]).
    async fn execute(
        &self,
        request: crate::host_envelope::AdapterExecuteRequest,
    ) -> Result<crate::ExecuteReply>;

    /// Same payload as [`Self::execute`]; kept for the Cap'n `executeEnvelope` ordinal.
    async fn execute_envelope(
        &self,
        envelope: crate::host_envelope::AdapterExecuteRequest,
    ) -> Result<crate::ExecuteReply> {
        self.execute(envelope).await
    }

    /// Commit.
    async fn commit(&self) -> Result<()>;

    /// Rollback.
    async fn rollback(&self) -> Result<()>;
}

/// Host-only view of an open adapter session (`begin` for interactive txn).
#[async_trait::async_trait(?Send)]
pub trait HostAdapterDatabaseSession {
    /// Opens a host-internal interactive transaction (SeaORM proxy).
    async fn begin(&self) -> Result<Box<dyn AdapterTransaction>>;

    /// Same payload as [`crate::AdapterDatabaseSession::execute`].
    async fn execute_envelope(
        &self,
        envelope: crate::host_envelope::AdapterExecuteRequest,
    ) -> Result<crate::ExecuteReply> {
        let _ = envelope;
        Err(crate::PluginError::unsupported(
            "host executeEnvelope not implemented",
        ))
    }
}
