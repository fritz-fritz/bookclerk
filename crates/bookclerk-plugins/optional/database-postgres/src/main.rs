//! PostgreSQL database plugin guest.

#![allow(clippy::missing_docs_in_private_items)]

use async_trait::async_trait;
use bookclerk_plugin_sdk::database_adapter::{
    guest_begin, guest_capabilities, guest_commit, guest_execute_atomic,
    guest_execute_atomic_on_txn, guest_rollback, plugin_error_from_engine, set_connection,
};
use bookclerk_plugin_sdk::legacy_db::DbConnectParams;
use bookclerk_plugin_sdk::v2::{
    AdapterDatabaseSession, AdapterTransaction, Database, DatabaseContext, PluginDescribe,
    PluginRoot, ScalarLimits, FEATURE_SCALAR_LIMITS, PRODUCT_API_VERSION,
};
use bookclerk_plugin_sdk::{
    serve, DbCapabilities, ExecuteReply, ExecuteRequest, HandshakeResult, PluginError,
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
}

struct PostgresSession;

#[async_trait(?Send)]
impl AdapterDatabaseSession for PostgresSession {
    async fn capabilities(&self) -> Result<DbCapabilities, PluginError> {
        guest_capabilities().await
    }

    async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteReply, PluginError> {
        guest_execute_atomic(request).await
    }

    async fn begin(&self) -> Result<Box<dyn AdapterTransaction>, PluginError> {
        let txn_id = guest_begin(None).await.map_err(plugin_error_from_engine)?;
        Ok(Box::new(PostgresTxn { txn_id }))
    }
}

struct PostgresTxn {
    txn_id: String,
}

#[async_trait(?Send)]
impl AdapterTransaction for PostgresTxn {
    async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteReply, PluginError> {
        guest_execute_atomic_on_txn(self.txn_id.clone(), request).await
    }

    async fn commit(&self) -> Result<(), PluginError> {
        guest_commit(self.txn_id.clone())
            .await
            .map_err(plugin_error_from_engine)
    }

    async fn rollback(&self) -> Result<(), PluginError> {
        guest_rollback(self.txn_id.clone())
            .await
            .map_err(plugin_error_from_engine)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    serve(PostgresRoot).await?;
    Ok(())
}
