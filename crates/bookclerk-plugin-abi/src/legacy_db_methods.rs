//! Host-private legacy JSON database RPC names (not in `METHOD_NAMES` / `abi.json`).

/// Run a read SQL statement through the database guest proxy.
pub mod db_query {
    /// Wire method name `"dbQuery"`.
    pub const NAME: &str = "dbQuery";
}

/// Run a write SQL statement through the database guest proxy.
pub mod db_execute {
    /// Wire method name `"dbExecute"`.
    pub const NAME: &str = "dbExecute";
}

/// Begin a database transaction (or nested savepoint) on the guest.
pub mod db_begin {
    /// Wire method name `"dbBegin"`.
    pub const NAME: &str = "dbBegin";
}

/// Commit a guest transaction previously returned by [`db_begin`].
pub mod db_commit {
    /// Wire method name `"dbCommit"`.
    pub const NAME: &str = "dbCommit";
}

/// Roll back a guest transaction previously returned by [`db_begin`].
pub mod db_rollback {
    /// Wire method name `"dbRollback"`.
    pub const NAME: &str = "dbRollback";
}

/// Run a host-authored generic SQL plan as one guest SQL transaction.
pub mod db_atomic {
    /// Wire method name `"dbAtomic"`.
    pub const NAME: &str = "dbAtomic";
}
