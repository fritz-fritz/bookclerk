//! Cloudflare D1 database plugin guest.

use async_trait::async_trait;
use bookclerk_db_guest::{
    guest_begin, guest_commit, guest_execute, guest_ping, guest_query, guest_rollback,
    set_connection,
};
use bookclerk_plugin_sdk::{
    BookclerkPlugin, BookclerkPluginGuest, DbBeginParams, DbBeginResult, DbConnectParams,
    DbConnectResult, DbTxnParams, DiagnoseResult, ExecResultDto, HandshakeParams, HandshakeResult,
    HealthResult, PluginError, QueryResultDto, StatementDto, PLUGIN_API_VERSION,
};

struct D1Plugin;

#[async_trait]
impl BookclerkPlugin for D1Plugin {
    async fn handshake(&self, _params: HandshakeParams) -> Result<HandshakeResult, PluginError> {
        Ok(HandshakeResult {
            api_version: PLUGIN_API_VERSION,
            id: "d1".into(),
            kind: "database".into(),
            display_name: Some("Cloudflare D1".into()),
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
            ],
            sort_key: Some(5),
            ..HandshakeResult::default()
        })
    }

    async fn health(&self) -> Result<HealthResult, PluginError> {
        Ok(HealthResult {
            ok: true,
            id: Some("d1".into()),
            enabled: Some(true),
            detail: Some("d1 database plugin ready".into()),
        })
    }

    async fn diagnose(&self) -> Result<DiagnoseResult, PluginError> {
        Ok(DiagnoseResult {
            lines: vec!["d1 database plugin diagnose: ok".into()],
        })
    }

    async fn db_connect(&self, params: DbConnectParams) -> Result<DbConnectResult, PluginError> {
        let DbConnectParams::D1 {
            plugin_data_dir: _,
            account_id,
            database_id,
            api_base,
            api_token,
        } = params
        else {
            return Err(PluginError::invalid_params(
                "d1 guest received non-d1 dbConnect params",
            ));
        };
        let db = bookclerk_plugin_database_d1::open(api_base, account_id, database_id, api_token)
            .await
            .map_err(|e| PluginError::internal(e.to_string()))?;
        set_connection(db).await;
        Ok(DbConnectResult::sqlite())
    }

    async fn db_ping(&self) -> Result<(), PluginError> {
        guest_ping().await.map_err(PluginError::internal)
    }

    async fn db_query(&self, params: StatementDto) -> Result<QueryResultDto, PluginError> {
        guest_query(params).await.map_err(PluginError::internal)
    }

    async fn db_execute(&self, params: StatementDto) -> Result<ExecResultDto, PluginError> {
        guest_execute(params).await.map_err(PluginError::internal)
    }

    async fn db_begin(&self, params: DbBeginParams) -> Result<DbBeginResult, PluginError> {
        let txn_id = guest_begin(params.parent_txn_id)
            .await
            .map_err(PluginError::internal)?;
        Ok(DbBeginResult { txn_id })
    }

    async fn db_commit(&self, params: DbTxnParams) -> Result<(), PluginError> {
        guest_commit(params.txn_id)
            .await
            .map_err(PluginError::internal)
    }

    async fn db_rollback(&self, params: DbTxnParams) -> Result<(), PluginError> {
        guest_rollback(params.txn_id)
            .await
            .map_err(PluginError::internal)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    BookclerkPluginGuest::serve(D1Plugin).await?;
    Ok(())
}
