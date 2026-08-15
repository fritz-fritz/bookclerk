//! Wrap a v1 [`BookclerkPlugin`] as a v2 [`PluginRoot`].
//!
//! JSON role methods are a migration bridge: existing storefront / integration /
//! database DTOs travel as bounded JSON on typed Cap'n Proto methods.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

use bookclerk_plugin_abi::{
    AuthenticateUserParams, CliInvokeParams, DbAtomicRequest, DbBeginParams, DbConnectParams,
    DbTxnParams, ExpandCandidatesParams, FetchTitleParams, HandshakeParams, HandshakeResult,
    HostToPluginEvent, ListAccountsParams, ListDealsParams, LoginCompleteParams, LoginParams,
    LoginStartParams, PluginError, PurchaseHintParams, ScanLibraryParams, ScanParams,
    SearchCatalogParams, StatementDto,
};

use super::{
    ContentSource, ContentSourceContext, Database, DatabaseContext, DatabaseSession, DomainEvent,
    EventResult, ExecResult, HealthOk, Integration, IntegrationContext, PluginDescribe, PluginRoot,
    QueryPage, ScalarLimits, Statement, Transaction, FEATURE_SCALAR_LIMITS, PRODUCT_API_VERSION,
};
use crate::plugin::BookclerkPlugin;

/// Native guest that implements [`PluginRoot`] by forwarding to [`BookclerkPlugin`].
pub struct V2PluginRoot<P: BookclerkPlugin> {
    inner: Arc<P>,
    handshake: Mutex<Option<HandshakeResult>>,
}

impl<P: BookclerkPlugin> V2PluginRoot<P> {
    /// Wraps a branded v1 guest implementation.
    #[must_use]
    pub fn new(plugin: P) -> Self {
        Self {
            inner: Arc::new(plugin),
            handshake: Mutex::new(None),
        }
    }

    /// Runs the wrapped handshake once and caches the result.
    ///
    /// # Errors
    ///
    /// Returns when the inner guest handshake fails.
    async fn ensure_handshake(&self, config_json: &str) -> Result<HandshakeResult, PluginError> {
        let mut slot = self.handshake.lock().await;
        if let Some(hs) = slot.as_ref() {
            return Ok(hs.clone());
        }
        let config = if config_json.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(config_json).unwrap_or_else(|_| Value::Object(Default::default()))
        };
        let hs = self
            .inner
            .handshake(HandshakeParams {
                api_version: PRODUCT_API_VERSION,
                config,
            })
            .await?;
        *slot = Some(hs.clone());
        Ok(hs)
    }
}

fn roles_for_kind(kind: &str) -> Vec<String> {
    match kind {
        "source" => vec!["contentSource".into()],
        "integration" => vec!["integration".into()],
        "database" => vec!["database".into()],
        "output" => vec!["destination".into(), "source".into(), "worker".into()],
        other => vec![other.to_string()],
    }
}

fn json_err(err: serde_json::Error) -> PluginError {
    PluginError::invalid_params(err.to_string())
}

/// Serialize `value` to JSON.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when serialization fails.
fn to_json<T: serde::Serialize>(value: T) -> Result<String, PluginError> {
    serde_json::to_string(&value).map_err(json_err)
}

/// Deserialize JSON into `T`, treating empty input as `{}`.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the payload is not valid JSON for `T`.
fn from_json<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, PluginError> {
    if json.trim().is_empty() {
        return serde_json::from_value(Value::Object(Default::default())).map_err(json_err);
    }
    serde_json::from_str(json).map_err(json_err)
}

#[async_trait(?Send)]
impl<P: BookclerkPlugin> PluginRoot for V2PluginRoot<P> {
    async fn describe(&self) -> Result<PluginDescribe, PluginError> {
        let hs = self.ensure_handshake("").await?;
        Ok(PluginDescribe {
            api_version: PRODUCT_API_VERSION,
            id: hs.id.clone(),
            kind: hs.kind.clone(),
            display_name: hs.display_name.clone(),
            rpc_features: vec![FEATURE_SCALAR_LIMITS.into()],
            scalar_limits: ScalarLimits::default().into(),
            supported_roles: roles_for_kind(&hs.kind),
            metadata_json: to_json(&hs)?,
            ..PluginDescribe::default()
        })
    }

    async fn content_source(
        &self,
        context: ContentSourceContext,
    ) -> Result<Box<dyn ContentSource>, PluginError> {
        self.ensure_handshake(&context.json).await?;
        Ok(Box::new(BridgedContentSource {
            inner: Arc::clone(&self.inner),
        }))
    }

    async fn integration(
        &self,
        context: IntegrationContext,
    ) -> Result<Box<dyn Integration>, PluginError> {
        self.ensure_handshake(&context.json).await?;
        Ok(Box::new(BridgedIntegration {
            inner: Arc::clone(&self.inner),
        }))
    }

    async fn database(&self, context: DatabaseContext) -> Result<Box<dyn Database>, PluginError> {
        self.ensure_handshake(&context.json).await?;
        if !context.json.trim().is_empty() {
            if let Ok(params) = serde_json::from_str::<DbConnectParams>(&context.json) {
                let _ = self.inner.db_connect(params).await?;
            }
        }
        Ok(Box::new(BridgedDatabase {
            inner: Arc::clone(&self.inner),
        }))
    }

    async fn cli_describe(&self) -> Result<String, PluginError> {
        to_json(self.inner.cli_describe().await?)
    }

    async fn cli_invoke(&self, params_json: &str) -> Result<String, PluginError> {
        let params: CliInvokeParams = from_json(params_json)?;
        to_json(self.inner.cli_invoke(params).await?)
    }

    async fn shutdown(&self) -> Result<(), PluginError> {
        self.inner.shutdown().await
    }
}

struct BridgedContentSource<P: BookclerkPlugin> {
    inner: Arc<P>,
}

#[async_trait(?Send)]
impl<P: BookclerkPlugin> ContentSource for BridgedContentSource<P> {
    async fn login(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .login(from_json::<LoginParams>(params_json)?)
                .await?,
        )
    }

    async fn scan(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .scan(from_json::<ScanParams>(params_json)?)
                .await?,
        )
    }

    async fn fetch_title(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .fetch_title(from_json::<FetchTitleParams>(params_json)?)
                .await?,
        )
    }

    async fn list_accounts(&self) -> Result<String, PluginError> {
        to_json(
            self.inner
                .list_accounts(ListAccountsParams::default())
                .await?,
        )
    }

    async fn login_start(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .login_start(from_json::<LoginStartParams>(params_json)?)
                .await?,
        )
    }

    async fn login_complete(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .login_complete(from_json::<LoginCompleteParams>(params_json)?)
                .await?,
        )
    }

    async fn search_catalog(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .search_catalog(from_json::<SearchCatalogParams>(params_json)?)
                .await?,
        )
    }

    async fn expand_candidates(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .expand_candidates(from_json::<ExpandCandidatesParams>(params_json)?)
                .await?,
        )
    }

    async fn purchase_hint(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .purchase_hint(from_json::<PurchaseHintParams>(params_json)?)
                .await?,
        )
    }

    async fn list_deals(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .list_deals(from_json::<ListDealsParams>(params_json)?)
                .await?,
        )
    }

    async fn health(&self) -> Result<HealthOk, PluginError> {
        let h = self.inner.health().await?;
        Ok(HealthOk {
            ok: h.ok,
            detail: h.detail.unwrap_or_default(),
        })
    }

    async fn diagnose(&self) -> Result<String, PluginError> {
        to_json(self.inner.diagnose().await?.lines)
    }
}

struct BridgedIntegration<P: BookclerkPlugin> {
    inner: Arc<P>,
}

/// Map a v2 [`DomainEvent`] onto the wrapped guest's host-event DTO.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the payload cannot be decoded.
fn domain_event_to_host(event: &DomainEvent) -> Result<HostToPluginEvent, PluginError> {
    if !event.payload.is_empty() {
        if let Ok(parsed) = serde_json::from_slice::<HostToPluginEvent>(&event.payload) {
            return Ok(parsed);
        }
        let wrapped = serde_json::json!({
            "type": event.event_type,
            "payload": serde_json::from_slice::<Value>(&event.payload).unwrap_or(Value::Null),
        });
        if let Ok(parsed) = serde_json::from_value::<HostToPluginEvent>(wrapped) {
            return Ok(parsed);
        }
    }
    serde_json::from_value(serde_json::json!({
        "type": event.event_type,
        "payload": {},
    }))
    .map_err(json_err)
}

#[async_trait(?Send)]
impl<P: BookclerkPlugin> Integration for BridgedIntegration<P> {
    async fn health(&self) -> Result<HealthOk, PluginError> {
        let h = self.inner.health().await?;
        Ok(HealthOk {
            ok: h.ok,
            detail: h.detail.unwrap_or_default(),
        })
    }

    async fn on_event(&self, event: DomainEvent) -> Result<EventResult, PluginError> {
        let host_event = domain_event_to_host(&event)?;
        self.inner.on_event(host_event).await?;
        Ok(EventResult::Ack)
    }

    async fn start(&self) -> Result<(), PluginError> {
        self.inner.start().await
    }

    async fn stop(&self) -> Result<(), PluginError> {
        self.inner.shutdown().await
    }

    async fn diagnose(&self) -> Result<String, PluginError> {
        to_json(self.inner.diagnose().await?.lines)
    }

    async fn scan_library(&self, params_json: &str) -> Result<(), PluginError> {
        self.inner
            .scan_library(from_json::<ScanLibraryParams>(params_json)?)
            .await
    }

    async fn sync_listening(&self) -> Result<String, PluginError> {
        to_json(self.inner.sync_listening().await?)
    }

    async fn authenticate_user(&self, params_json: &str) -> Result<String, PluginError> {
        to_json(
            self.inner
                .authenticate_user(from_json::<AuthenticateUserParams>(params_json)?)
                .await?,
        )
    }

    async fn poll_events(&self) -> Result<String, PluginError> {
        to_json(self.inner.poll_events().await?)
    }
}

struct BridgedDatabase<P: BookclerkPlugin> {
    inner: Arc<P>,
}

#[async_trait(?Send)]
impl<P: BookclerkPlugin> Database for BridgedDatabase<P> {
    async fn open_session(&self) -> Result<Box<dyn DatabaseSession>, PluginError> {
        Ok(Box::new(BridgedSession {
            inner: Arc::clone(&self.inner),
        }))
    }
}

struct BridgedSession<P: BookclerkPlugin> {
    inner: Arc<P>,
}

fn to_dto(statement: &Statement, txn_id: Option<String>) -> StatementDto {
    StatementDto {
        sql: statement.sql.clone(),
        values: serde_json::from_str(&statement.values_json).unwrap_or_default(),
        txn_id,
    }
}

fn exec_from_dto(dto: bookclerk_plugin_abi::ExecResultDto) -> ExecResult {
    ExecResult {
        last_insert_id: i64::try_from(dto.last_insert_id).unwrap_or(i64::MAX),
        rows_affected: dto.rows_affected,
    }
}

#[async_trait(?Send)]
impl<P: BookclerkPlugin> DatabaseSession for BridgedSession<P> {
    async fn execute(&self, statement: Statement) -> Result<ExecResult, PluginError> {
        if statement.sql == "bookclerk.atomic" {
            return Err(PluginError::unsupported(
                "bookclerk.atomic is a query, not execute",
            ));
        }
        let dto = self.inner.db_execute(to_dto(&statement, None)).await?;
        Ok(exec_from_dto(dto))
    }

    async fn query(
        &self,
        statement: Statement,
        _cursor: &str,
        _limit: u32,
    ) -> Result<QueryPage, PluginError> {
        if statement.sql == "bookclerk.atomic" {
            let req: DbAtomicRequest = from_json(&statement.values_json)?;
            let result = self.inner.db_atomic(req).await?;
            return Ok(QueryPage {
                rows_json: serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()),
                next_cursor: None,
            });
        }
        let dto = self.inner.db_query(to_dto(&statement, None)).await?;
        Ok(QueryPage {
            rows_json: serde_json::to_string(&dto.rows).unwrap_or_else(|_| "[]".into()),
            next_cursor: None,
        })
    }

    async fn begin(&self) -> Result<Box<dyn Transaction>, PluginError> {
        let result = self
            .inner
            .db_begin(DbBeginParams {
                parent_txn_id: None,
            })
            .await?;
        Ok(Box::new(BridgedTxn {
            inner: Arc::clone(&self.inner),
            txn_id: result.txn_id,
        }))
    }
}

struct BridgedTxn<P: BookclerkPlugin> {
    inner: Arc<P>,
    txn_id: String,
}

#[async_trait(?Send)]
impl<P: BookclerkPlugin> Transaction for BridgedTxn<P> {
    async fn execute(&self, statement: Statement) -> Result<ExecResult, PluginError> {
        let dto = self
            .inner
            .db_execute(to_dto(&statement, Some(self.txn_id.clone())))
            .await?;
        Ok(exec_from_dto(dto))
    }

    async fn query(
        &self,
        statement: Statement,
        _cursor: &str,
        _limit: u32,
    ) -> Result<QueryPage, PluginError> {
        let dto = self
            .inner
            .db_query(to_dto(&statement, Some(self.txn_id.clone())))
            .await?;
        Ok(QueryPage {
            rows_json: serde_json::to_string(&dto.rows).unwrap_or_else(|_| "[]".into()),
            next_cursor: None,
        })
    }

    async fn commit(&self) -> Result<(), PluginError> {
        self.inner
            .db_commit(DbTxnParams {
                txn_id: self.txn_id.clone(),
            })
            .await
    }

    async fn rollback(&self) -> Result<(), PluginError> {
        self.inner
            .db_rollback(DbTxnParams {
                txn_id: self.txn_id.clone(),
            })
            .await
    }
}
