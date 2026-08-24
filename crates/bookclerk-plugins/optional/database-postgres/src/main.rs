//! PostgreSQL database plugin guest.

#![allow(clippy::missing_docs_in_private_items)]

use async_trait::async_trait;
use bookclerk_plugin_sdk::database_adapter::{
    guest_capabilities, guest_execute_atomic, host_session, set_connection,
};
use bookclerk_plugin_sdk::v2::{
    AdapterDatabaseSession, Database, DatabaseContext, HostAdapterDatabaseSession, PluginDescribe,
    PluginRoot, ScalarLimits, FEATURE_SCALAR_LIMITS, PRODUCT_API_VERSION,
};
use bookclerk_plugin_sdk::DbConnectParams;
use bookclerk_plugin_sdk::{
    serve, DbCapabilities, ExecuteReply, ExecuteRequest, GuestReceiptPersist, HandshakeResult,
    HostExecuteEnvelope, PluginError,
};

fn describe_metadata() -> Result<String, PluginError> {
    bookclerk_plugin_sdk::encode_json(HandshakeResult {
        api_version: PRODUCT_API_VERSION,
        id: "postgres".into(),
        kind: "database".into(),
        display_name: Some("PostgreSQL".into()),
        capabilities: vec![
            "health".into(),
            "diagnose".into(),
            "dbConnect".into(),
            "dbPing".into(),
            "dbQuery".into(),
            "dbExecute".into(),
            "dbBegin".into(),
            "dbCommit".into(),
            "dbRollback".into(),
            "dbAtomic".into(),
        ],
        sort_key: Some(5),
        ..HandshakeResult::default()
    })
}

async fn connect_from_context(ctx: &DatabaseContext) -> Result<(), PluginError> {
    let params: DbConnectParams = if ctx.json.trim().is_empty() {
        return Err(PluginError::invalid_params(
            "postgres database context is missing connect params",
        ));
    } else {
        serde_json::from_str(&ctx.json)
            .map_err(|err| PluginError::invalid_params(err.to_string()))?
    };
    let DbConnectParams::Postgres {
        plugin_data_dir: _,
        url,
    } = params
    else {
        return Err(PluginError::invalid_params(
            "postgres guest received non-postgres database context",
        ));
    };
    let db = bookclerk_plugin_database_postgres::open(&url)
        .await
        .map_err(|e| PluginError::internal(e.to_string()))?;
    set_connection(db).await;
    Ok(())
}

/// Database guest that opens a Postgres URL and serves typed adapter RPC.
struct PostgresRoot;

#[async_trait(?Send)]
impl PluginRoot for PostgresRoot {
    async fn describe(&self) -> Result<PluginDescribe, PluginError> {
        Ok(PluginDescribe {
            api_version: PRODUCT_API_VERSION,
            id: "postgres".into(),
            kind: "database".into(),
            display_name: Some("PostgreSQL".into()),
            rpc_features: vec![FEATURE_SCALAR_LIMITS.into()],
            scalar_limits: ScalarLimits::default().into(),
            supported_roles: vec!["database".into()],
            metadata_json: describe_metadata()?,
            ..PluginDescribe::default()
        })
    }

    async fn database(&self, context: DatabaseContext) -> Result<Box<dyn Database>, PluginError> {
        connect_from_context(&context).await?;
        Ok(Box::new(PostgresDatabase))
    }
}

struct PostgresDatabase;

#[async_trait(?Send)]
impl Database for PostgresDatabase {
    async fn open_session(&self) -> Result<Box<dyn AdapterDatabaseSession>, PluginError> {
        Ok(Box::new(PostgresSession))
    }

    async fn host_adapter_session(
        &self,
    ) -> Result<Option<Box<dyn HostAdapterDatabaseSession>>, PluginError> {
        Ok(Some(Box::new(host_session())))
    }
}

struct PostgresSession;

#[async_trait(?Send)]
impl AdapterDatabaseSession for PostgresSession {
    async fn capabilities(&self) -> Result<DbCapabilities, PluginError> {
        guest_capabilities().await
    }

    async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteReply, PluginError> {
        guest_execute_atomic(HostExecuteEnvelope::new(
            request,
            GuestReceiptPersist::default(),
        ))
        .await
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    serve(PostgresRoot).await?;
    Ok(())
}
