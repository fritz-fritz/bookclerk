//! ABI v2 database factory for the platform SQLite guest.

#![allow(
    clippy::missing_docs_in_private_items,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use async_trait::async_trait;
use bookclerk_db_guest::{
    guest_bootstrap, guest_capabilities, guest_execute_atomic, host_session, set_connection,
};
use bookclerk_plugin_abi::db::{connect_params_from_context, DbConnectParams};
use bookclerk_plugin_abi::v2::AdapterSessionOpen;
use bookclerk_plugin_abi::{GuestReceiptPersist, HostExecuteEnvelope};
use bookclerk_plugin_sdk::v2::{
    AdapterDatabaseSession, Database, DatabaseContext, PluginDescribe, PluginRoot, ScalarLimits,
    FEATURE_SCALAR_LIMITS, PRODUCT_API_VERSION,
};
use bookclerk_plugin_sdk::{
    DbBootstrap, DbCapabilities, ExecuteReply, ExecuteRequest, PluginError,
};

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
            connect_params_from_context(ctx).ok().and_then(|params| {
                let DbConnectParams::Sqlite { sqlite_path, .. } = params else {
                    return None;
                };
                sqlite_path
            })
        })
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
    async fn open_session(&self) -> Result<AdapterSessionOpen> {
        Ok(AdapterSessionOpen::with_host(
            Box::new(SqliteSession),
            Box::new(host_session()),
        ))
    }
}

struct SqliteSession;

#[async_trait(?Send)]
impl AdapterDatabaseSession for SqliteSession {
    async fn capabilities(&self) -> Result<DbCapabilities> {
        guest_capabilities().await
    }

    async fn bootstrap(&self) -> Result<DbBootstrap> {
        guest_bootstrap().await
    }

    async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteReply> {
        guest_execute_atomic(HostExecuteEnvelope::new(
            request,
            GuestReceiptPersist::default(),
        ))
        .await
    }
}
