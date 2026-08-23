//! ABI v2 database factory for the platform SQLite guest.

#![allow(clippy::missing_docs_in_private_items)]

use async_trait::async_trait;
use bookclerk_plugin_sdk::database_adapter::{
    guest_begin, guest_capabilities, guest_commit, guest_execute_atomic,
    guest_execute_atomic_on_txn, guest_rollback, plugin_error_from_engine, set_connection,
};
use bookclerk_plugin_sdk::v2::{
    AdapterDatabaseSession, AdapterTransaction, Database, DatabaseContext, PluginDescribe,
    PluginRoot, ScalarLimits, FEATURE_SCALAR_LIMITS, PRODUCT_API_VERSION,
};
use bookclerk_plugin_sdk::{DbCapabilities, ExecuteReply, ExecuteRequest, PluginError};

use crate::ID;

type Result<T> = std::result::Result<T, PluginError>;

/// Root capability for the platform SQLite database guest.
pub struct SqliteRoot;

#[async_trait(?Send)]
impl PluginRoot for SqliteRoot {
    async fn describe(&self) -> Result<PluginDescribe> {
        Ok(PluginDescribe {
            api_version: PRODUCT_API_VERSION,
            id: ID.into(),
            kind: "database".into(),
            display_name: Some("SQLite".into()),
            rpc_features: vec![FEATURE_SCALAR_LIMITS.into()],
            scalar_limits: ScalarLimits::default().into(),
            supported_roles: vec!["database".into()],
            ..PluginDescribe::default()
        })
    }

    async fn database(&self, context: DatabaseContext) -> Result<Box<dyn Database>> {
        connect_from_context(&context).await?;
        Ok(Box::new(SqliteDatabase))
    }
}

async fn connect_from_context(ctx: &DatabaseContext) -> Result<()> {
    let path = std::env::var("BOOKCLERK_SQLITE_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&ctx.json)
                .ok()
                .and_then(|v| {
                    v.get("sqlitePath")
                        .and_then(|p| p.as_str())
                        .map(str::to_string)
                })
        })
        .unwrap_or_default();
    if path.is_empty() {
        return Err(PluginError::internal(
            "sqlite database path missing (BOOKCLERK_SQLITE_PATH or context sqlitePath)",
        ));
    }
    let db = crate::open(std::path::Path::new(&path))
        .await
        .map_err(|e| PluginError::internal(e.to_string()))?;
    set_connection(db).await;
    Ok(())
}

struct SqliteDatabase;

#[async_trait(?Send)]
impl Database for SqliteDatabase {
    async fn open_session(&self) -> Result<Box<dyn AdapterDatabaseSession>> {
        Ok(Box::new(SqliteSession))
    }
}

struct SqliteSession;

#[async_trait(?Send)]
impl AdapterDatabaseSession for SqliteSession {
    async fn capabilities(&self) -> Result<DbCapabilities> {
        guest_capabilities().await
    }

    async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteReply> {
        guest_execute_atomic(request).await
    }

    async fn begin(&self) -> Result<Box<dyn AdapterTransaction>> {
        let txn_id = guest_begin(None).await.map_err(plugin_error_from_engine)?;
        Ok(Box::new(SqliteTxn { txn_id }))
    }
}

struct SqliteTxn {
    txn_id: String,
}

#[async_trait(?Send)]
impl AdapterTransaction for SqliteTxn {
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
