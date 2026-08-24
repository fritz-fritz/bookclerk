//! Host-private ABI v2 roles (not exposed to third-party plugin authors).

use crate::Result;

/// Host-internal transaction on an adapter connection (SeaORM proxy).
#[async_trait::async_trait(?Send)]
pub trait AdapterTransaction {
    /// Typed statements on this open transaction (no second `BEGIN`).
    async fn execute(&self, request: crate::ExecuteRequest) -> Result<crate::ExecuteReply>;

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
}
