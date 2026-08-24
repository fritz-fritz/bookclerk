//! [`ProxyDatabaseTrait`] adapter over an external database plugin process.
//!
//! The host mediates credentials, applies schema after connect, and forwards
//! SeaORM proxy calls over JSON-RPC. Engine connect/proxy quirks live in the
//! database guest. There is no in-process fallback.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bookclerk_config::{resolve_d1_api_token, resolve_postgres_url, Config, DatabasePluginKind};
use bookclerk_plugin_sdk::legacy_db::{
    db_value_from_sea, exec_result_from_dto, proxy_rows_from_typed, DbAtomicPlan, DbAtomicRequest,
    DbConnectParams, DbConnectResult, DbPlanStatement, DbPlanStatementKind, ExecResultDto,
};
use bookclerk_plugin_sdk::v2::GuestDatabase;
use bookclerk_plugin_sdk::v2::PRODUCT_API_VERSION;
use bookclerk_plugin_sdk::{
    DbResultSelection, ExecuteReply, ExecuteRequest, PluginError as AbiPluginError,
    TypedDbStatement,
};
use sea_orm::{
    Database, DatabaseConnection, DbBackend, DbErr, ProxyDatabaseTrait, ProxyExecResult, ProxyRow,
    Statement,
};
use serde_json::Value;
use tokio::task::{try_id, Id as TaskId};

use crate::discover::DiscoveredPlugin;
use crate::jail::plugin_data_dir;
use crate::rpc_v2::{V2PluginSession, OPERATOR_ACCOUNT};
use crate::{PluginError, Result as PluginResult};
use bookclerk_library::{atomic_status, DbAtomicParams};

/// External database backend spawned for `[database].plugin`.
#[derive(Clone)]
pub struct ExternalDatabase {
    /// Cap'n Proto v2 session (vat holds the database session).
    session: Arc<V2PluginSession>,
    /// Manifest id (`sqlite`, `d1`, `postgres`) used to build connect params.
    plugin_id: String,
    /// Guest HOME / data directory passed on `db.connect`.
    plugin_data_dir: std::path::PathBuf,
}

impl ExternalDatabase {
    /// Spawn and handshake a database plugin (connection happens later).
    ///
    /// # Errors
    ///
    /// Returns an error when the operation fails.
    pub async fn spawn(plugin: &DiscoveredPlugin, config: &Config) -> PluginResult<Self> {
        if plugin.manifest.api_version != PRODUCT_API_VERSION {
            return Err(PluginError::message(format!(
                "plugin `{}` api_version {} is not v2",
                plugin.manifest.id, plugin.manifest.api_version
            )));
        }
        let table = crate::settings_table(config, plugin);
        let config_json = toml_to_json(&toml::Value::Table(table));
        let plugin_data_dir = plugin_data_dir(config, &plugin.manifest.id)?;
        let extra_env = match DatabasePluginKind::parse(&plugin.manifest.id) {
            Some(DatabasePluginKind::D1) | Some(DatabasePluginKind::Postgres) => Vec::new(),
            Some(DatabasePluginKind::Sqlite) | None => {
                let path = config.database.sqlite_path(&config.paths().files_dir);
                vec![(
                    "BOOKCLERK_SQLITE_PATH",
                    std::ffi::OsString::from(path.as_os_str()),
                )]
            }
        };
        let session = Arc::new(
            V2PluginSession::spawn_for_account_with_env(
                plugin,
                config,
                config_json,
                OPERATOR_ACCOUNT,
                extra_env.as_slice(),
            )
            .await?,
        );
        Ok(Self {
            session,
            plugin_id: plugin.manifest.id.clone(),
            plugin_data_dir,
        })
    }

    /// Open the library connection through the guest (`database.openSession`).
    ///
    /// # Errors
    ///
    /// Returns an error when the operation fails.
    pub async fn connect(
        &self,
        config: &Config,
    ) -> Result<(DatabaseConnection, DbConnectResult), DbErr> {
        let params = connect_params(
            config,
            &self.plugin_id,
            &self.plugin_data_dir,
            &self.session,
        )?;
        let ctx = serde_json::to_string(&params).map_err(|err| DbErr::Custom(err.to_string()))?;
        self.session.db_open(ctx).await.map_err(map_rpc_err)?;

        let caps = self.session.db_capabilities().await.map_err(map_rpc_err)?;
        if !caps.meets_host_minimums() {
            return Err(DbErr::Custom(caps.capability_failure_reason()));
        }
        let _kind = bookclerk_library::HostSchemaKind::from_db_capabilities(&caps)
            .map_err(|err| DbErr::Custom(err.to_string()))?;
        let connect_result = caps.to_connect();
        if let Some(reason) = connect_result.bootstrap_backend_failure_reason() {
            return Err(DbErr::Custom(reason));
        }
        let backend = seaorm_backend_from_connect(&connect_result)?;
        let proxy: Arc<Box<dyn ProxyDatabaseTrait>> = Arc::new(Box::new(RpcDatabaseProxy {
            session: self.session.clone(),
            txn_depth: Arc::new(Mutex::new(HashMap::new())),
            caps: connect_result.clone(),
        }));
        let db = Database::connect_proxy(backend, proxy).await?;
        self.apply_host_schema(&db, &connect_result).await?;
        Ok((db, connect_result))
    }

    /// Reads the guest schema version and applies remaining host-authored DDL.
    async fn apply_host_schema(
        &self,
        db: &DatabaseConnection,
        caps: &DbConnectResult,
    ) -> Result<(), DbErr> {
        let kind = bookclerk_library::HostSchemaKind::from_capabilities(caps)
            .map_err(|err| DbErr::Custom(err.to_string()))?;
        let session = self.session.clone();
        let caps = caps.clone();
        bookclerk_library::apply_host_schema_with_batch(db, kind, move |stmts| {
            let session = session.clone();
            let caps = caps.clone();
            async move { exec_host_ddl_batch(&session, &caps, stmts).await }
        })
        .await
        .map_err(|err| DbErr::Custom(err.to_string()))
    }
}

/// Long-lived external database plugin for the active `[database].plugin`.
#[derive(Default, Clone)]
pub struct DatabaseRegistry {
    /// Spawned guest matching `[database].plugin`, if handshake succeeded.
    active: Option<Arc<ExternalDatabase>>,
}

impl DatabaseRegistry {
    /// Currently active external database guest for `[database].plugin`.
    #[must_use]
    pub fn active(&self) -> Option<Arc<ExternalDatabase>> {
        self.active.clone()
    }
}

/// Discover and spawn the external database plugin matching `[database].plugin`.
///
/// Guests are required: when the matching database plugin is missing or fails to
/// start, [`open_library_store`] returns an error (no in-process engine).
///
/// # Errors
///
/// Returns an error when the operation fails.
pub async fn load_external_database(config: &Config) -> PluginResult<DatabaseRegistry> {
    let mut registry = DatabaseRegistry::default();
    let active = config.database.plugin.trim().to_ascii_lowercase();
    for plugin in crate::discover_plugins(config)? {
        if plugin.manifest.kind != crate::PluginKind::Database {
            continue;
        }
        if plugin.manifest.id.to_ascii_lowercase() != active {
            continue;
        }
        match ExternalDatabase::spawn(&plugin, config).await {
            Ok(db) => {
                tracing::info!(
                    id = %plugin.manifest.id,
                    path = %plugin.command.display(),
                    "loaded external database plugin"
                );
                registry.active = Some(Arc::new(db));
            }
            Err(err) => {
                return Err(PluginError::Other(anyhow::anyhow!(
                    "failed to start database plugin `{active}`: {err}"
                )));
            }
        }
        break;
    }
    if registry.active.is_none() {
        let files = &config.paths().files_dir;
        let expected = files.join("plugins").join(&active);
        let mut hint = format!(
            "looked under {} (set {} to the directory `cargo dev` uses, \
             typically ./BookclerkFiles, or run `cargo install-platform` / \
             `cargo dev-cli -- daemon token`)",
            expected.display(),
            bookclerk_config::BOOKCLERK_FILES_DIR_ENV
        );
        if let Ok(cwd) = std::env::current_dir() {
            let alt = cwd.join("BookclerkFiles").join("plugins").join(&active);
            if alt.is_dir() && alt != expected {
                hint.push_str(&format!(
                    "; found guest at {} — export {}={}",
                    alt.display(),
                    bookclerk_config::BOOKCLERK_FILES_DIR_ENV,
                    cwd.join("BookclerkFiles").display()
                ));
            }
        }
        return Err(PluginError::Other(anyhow::anyhow!(
            "database plugin `{active}` not found — {hint} (see docs/database.md)"
        )));
    }
    Ok(registry)
}

/// Open [`bookclerk_library::LibraryStore`] via the external database guest (required).
///
/// # Errors
///
/// Returns an error when the operation fails.
pub async fn open_library_store(
    config: &Config,
    registry: &DatabaseRegistry,
) -> bookclerk_library::Result<bookclerk_library::LibraryStore> {
    let ext = registry.active().ok_or_else(|| {
        bookclerk_library::LibraryError::Other(anyhow::anyhow!(
            "no active database plugin — stage and enable [database].plugin"
        ))
    })?;
    let (db, caps) = ext
        .connect(config)
        .await
        .map_err(bookclerk_library::LibraryError::Orm)?;
    let backend = Arc::new(RpcAtomicBackend {
        session: ext.session.clone(),
        caps: caps.clone(),
    });
    let store = bookclerk_library::LibraryStore::from_connection(db)
        .with_connect_result(caps)
        .with_atomic_txn(backend.clone())
        .with_typed_exec(backend);
    store.ensure_users_bridged().await?;
    Ok(store)
}

/// Granted `JobHandler.handle` database session for one library store.
///
/// SQL goes through
/// [`LibraryStore::execute_guest_atomic`](bookclerk_library::LibraryStore::execute_guest_atomic)
/// with a host-issued table grant (`books`). Unrelated Bookclerk tables stay
/// denied. Workerd HTTP grants defer the same check to this session.
#[must_use]
pub(crate) fn granted_job_database(
    store: bookclerk_library::LibraryStore,
) -> Arc<dyn GuestDatabase> {
    granted_job_database_with_policy(
        store,
        bookclerk_library::GuestSqlPolicy::allow_tables(["books"]),
    )
}

/// Like [`granted_job_database`], with an explicit table/column/function policy.
#[must_use]
pub(crate) fn granted_job_database_with_policy(
    store: bookclerk_library::LibraryStore,
    policy: bookclerk_library::GuestSqlPolicy,
) -> Arc<dyn GuestDatabase> {
    Arc::new(GuestJobDatabase { store, policy })
}

/// Host-exported [`GuestDatabase`] for one `JobHandler.handle` invocation.
struct GuestJobDatabase {
    /// Library used for authorized typed `execute`.
    store: bookclerk_library::LibraryStore,
    /// Host-issued table/column/function allowlist for this job.
    policy: bookclerk_library::GuestSqlPolicy,
}

#[async_trait(?Send)]
impl GuestDatabase for GuestJobDatabase {
    async fn execute(
        &self,
        request: ExecuteRequest,
    ) -> std::result::Result<ExecuteReply, AbiPluginError> {
        self.store.execute_guest_atomic(request, &self.policy).await
    }
}

/// Open the library for a specific `[database].plugin` id (ignoring the active config value).
///
/// # Errors
///
/// Returns an error when the operation fails.
pub async fn open_library_store_for_plugin(
    config: &Config,
    plugin_id: &str,
) -> bookclerk_library::Result<bookclerk_library::LibraryStore> {
    let mut cfg = config.clone();
    cfg.database.plugin = plugin_id.trim().to_string();
    let registry = load_external_database(&cfg)
        .await
        .map_err(|err| bookclerk_library::LibraryError::Other(anyhow::anyhow!(err.to_string())))?;
    open_library_store(&cfg, &registry).await
}

/// Copy library data from one database plugin backend to another.
///
/// # Errors
///
/// Returns an error when the operation fails.
pub async fn migrate_database_plugin(
    config: &Config,
    from_plugin: &str,
    to_plugin: &str,
    opts: &bookclerk_library::BackendMigrateOptions,
) -> bookclerk_library::Result<bookclerk_library::BackendMigrateSummary> {
    if from_plugin.eq_ignore_ascii_case(to_plugin) {
        return Err(bookclerk_library::LibraryError::Other(anyhow::anyhow!(
            "source and destination database plugins are both `{from_plugin}`"
        )));
    }
    let source = open_library_store_for_plugin(config, from_plugin).await?;
    let dest = open_library_store_for_plugin(config, to_plugin).await?;
    bookclerk_library::migrate_library_backend(source.db(), dest.db(), opts).await
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
/// Per-task identity used to stack nested SeaORM / guest transaction ids.
enum TaskKey {
    /// Tokio task id when the proxy runs inside a runtime task.
    Tokio(TaskId),
    /// OS thread id when no Tokio task id is available (blocking paths).
    Thread(ThreadId),
}

/// Current Tokio task id, or the OS thread id outside a task.
fn task_key() -> TaskKey {
    match try_id() {
        Some(id) => TaskKey::Tokio(id),
        None => TaskKey::Thread(std::thread::current().id()),
    }
}

#[derive(Clone)]
/// SeaORM [`ProxyDatabaseTrait`] that forwards query/exec/txn RPCs to the guest.
struct RpcDatabaseProxy {
    /// Cap'n Proto v2 session shared with [`ExternalDatabase`].
    session: Arc<V2PluginSession>,
    /// Per-task nested begin depth (vat holds a single transaction).
    txn_depth: Arc<Mutex<HashMap<TaskKey, usize>>>,
    /// Negotiated guest capabilities (statement/bind/request byte limits).
    caps: DbConnectResult,
}

impl std::fmt::Debug for RpcDatabaseProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcDatabaseProxy").finish_non_exhaustive()
    }
}

impl RpcDatabaseProxy {
    /// Locks the per-task txn depth map, recovering a poisoned mutex.
    fn lock_depth(&self) -> std::sync::MutexGuard<'_, HashMap<TaskKey, usize>> {
        self.txn_depth.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Current nested begin depth for this task.
    fn depth(&self) -> usize {
        self.lock_depth().get(&task_key()).copied().unwrap_or(0)
    }

    /// Increments depth; returns the previous depth.
    fn push_depth(&self) -> usize {
        let mut map = self.lock_depth();
        let entry = map.entry(task_key()).or_insert(0);
        let prev = *entry;
        *entry = prev.saturating_add(1);
        prev
    }

    /// Decrements depth; returns the depth after decrement (`0` means fully closed).
    fn pop_depth(&self) -> Option<usize> {
        let mut map = self.lock_depth();
        let key = task_key();
        let entry = map.get_mut(&key)?;
        if *entry == 0 {
            map.remove(&key);
            return None;
        }
        *entry -= 1;
        let next = *entry;
        if next == 0 {
            map.remove(&key);
        }
        Some(next)
    }

    /// Serializes a SeaORM statement into a typed `ExecuteRequest` statement.
    ///
    /// # Errors
    ///
    /// Returns when a SeaORM bind is outside the universal `DbValue` domain.
    fn statement_to_typed(
        statement: &Statement,
        kind: DbPlanStatementKind,
        selection: DbResultSelection,
    ) -> Result<TypedDbStatement, DbErr> {
        let parameters = match &statement.values {
            Some(values) => values
                .0
                .iter()
                .map(db_value_from_sea)
                .collect::<Result<Vec<_>, _>>()
                .map_err(DbErr::Custom)?,
            None => Vec::new(),
        };
        Ok(TypedDbStatement {
            sql: statement.sql.clone(),
            parameters,
            kind,
            max_rows: 0,
            result_selection: selection,
        })
    }

    /// One-statement `ExecuteRequest` used by the proxy query/execute/ping path.
    fn typed_request(stmt: TypedDbStatement, operation_id: String) -> ExecuteRequest {
        ExecuteRequest {
            operation_id,
            request_hash: String::new(),
            statements: vec![stmt],
            deadline_unix_ms: 0,
        }
    }

    /// Validates `req` against negotiated caps, stamps the host request hash,
    /// then sends `executeAtomic`.
    ///
    /// Guest-supplied statement kinds are replaced with host-authored kinds
    /// derived from the SQL. After validation the host stamps the canonical
    /// Cap'n request hash. A non-empty guest hash must match that digest
    /// (explicit retry token).
    ///
    /// # Errors
    ///
    /// Returns a plugin ABI `invalid_params` error when the request exceeds
    /// negotiated caps or the retry hash does not match, and a plugin error
    /// when `executeAtomic` fails.
    async fn execute_typed_validated(
        &self,
        mut req: ExecuteRequest,
    ) -> Result<bookclerk_plugin_sdk::ExecuteReply, crate::PluginError> {
        bookclerk_library::authorize_typed_request(&mut req, &self.caps)
            .map_err(|err| crate::PluginError::from_abi(Some("invalid_params"), err.to_string()))?;
        let validate_req = req.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let reply = if self.depth() > 0 {
            self.session.db_txn_execute_request(req, cancel).await
        } else {
            self.session.db_execute_request(req, cancel).await
        }?;
        bookclerk_library::validate_execute_reply(&validate_req, &reply, &self.caps)
            .map_err(map_reply_validation_err)?;
        Ok(reply)
    }
}

#[async_trait]
impl ProxyDatabaseTrait for RpcDatabaseProxy {
    async fn query(&self, statement: Statement) -> std::result::Result<Vec<ProxyRow>, DbErr> {
        if bookclerk_library::is_txn_broken() {
            return Err(bookclerk_library::txn_broken_err());
        }
        let typed = Self::statement_to_typed(
            &statement,
            bookclerk_library::proxy_read_kind(&statement.sql),
            DbResultSelection::Rows,
        )?;
        let req = Self::typed_request(typed, format!("proxy-query-{}", uuid::Uuid::new_v4()));
        let reply = self
            .execute_typed_validated(req)
            .await
            .map_err(map_rpc_err)?;
        let stmt =
            reply.statements.into_iter().next().ok_or_else(|| {
                DbErr::Custom("execute query returned no statement result".into())
            })?;
        proxy_rows_from_typed(&stmt).map_err(DbErr::Custom)
    }

    async fn execute(&self, statement: Statement) -> std::result::Result<ProxyExecResult, DbErr> {
        if bookclerk_library::is_txn_broken() {
            return Err(bookclerk_library::txn_broken_err());
        }
        let typed = Self::statement_to_typed(
            &statement,
            bookclerk_library::proxy_write_kind(&statement.sql),
            DbResultSelection::AffectedRows,
        )?;
        let req = Self::typed_request(typed, format!("proxy-exec-{}", uuid::Uuid::new_v4()));
        let reply = self
            .execute_typed_validated(req)
            .await
            .map_err(map_rpc_err)?;
        let rows_affected = reply
            .statements
            .first()
            .map(|s| s.rows_affected)
            .unwrap_or(0);
        Ok(exec_result_from_dto(ExecResultDto {
            last_insert_id: 0,
            rows_affected,
        }))
    }

    async fn ping(&self) -> std::result::Result<(), DbErr> {
        let typed = TypedDbStatement {
            sql: "SELECT 1".into(),
            parameters: Vec::new(),
            kind: DbPlanStatementKind::Select,
            max_rows: 1,
            result_selection: DbResultSelection::Rows,
        };
        let req =
            RpcDatabaseProxy::typed_request(typed, format!("proxy-ping-{}", uuid::Uuid::new_v4()));
        self.execute_typed_validated(req)
            .await
            .map_err(map_rpc_err)?;
        Ok(())
    }

    async fn begin(&self) {
        if bookclerk_library::consume_begin_injection() {
            bookclerk_library::note_begin_failed("injected begin failure");
            return;
        }
        let prev = self.push_depth();
        if prev != 0 {
            return;
        }
        if let Err(err) = self.session.db_begin().await {
            self.pop_depth();
            bookclerk_library::note_begin_failed(&err);
            tracing::error!(error = %err, "database plugin dbBegin failed");
        }
    }

    async fn commit(&self) {
        if bookclerk_library::consume_commit_injection() {
            if self.pop_depth().is_some() && self.depth() == 0 {
                if let Err(err) = self.session.db_rollback().await {
                    tracing::error!(
                        error = %err,
                        "database plugin dbRollback after injected commit failure"
                    );
                }
            }
            bookclerk_library::note_commit_failed("injected commit failure");
            return;
        }
        let Some(next) = self.pop_depth() else {
            if bookclerk_library::is_txn_broken() {
                return;
            }
            bookclerk_library::note_commit_failed("no open transaction to commit");
            return;
        };
        if next != 0 {
            return;
        }
        if let Err(err) = self.session.db_commit().await {
            bookclerk_library::note_commit_failed(&err);
            tracing::error!(error = %err, "database plugin dbCommit failed");
            if let Err(rb) = self.session.db_rollback().await {
                tracing::error!(error = %rb, "database plugin dbRollback after commit failure");
            }
        }
    }

    async fn rollback(&self) {
        let Some(next) = self.pop_depth() else {
            return;
        };
        if next != 0 {
            return;
        }
        if let Err(err) = self.session.db_rollback().await {
            tracing::error!(error = %err, "database plugin dbRollback failed");
        }
    }

    fn start_rollback(&self) {
        let Some(next) = self.pop_depth() else {
            return;
        };
        if next != 0 {
            return;
        }
        let session = self.session.clone();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::error!("database plugin dbRollback skipped: no tokio runtime");
            return;
        };
        if let Err(err) = tokio::task::block_in_place(|| {
            handle.block_on(async move { session.db_rollback().await.map(|_| ()) })
        }) {
            tracing::error!(error = %err, "database plugin dbRollback failed");
        }
    }
}

/// Host [`AtomicTxnBackend`] that runs named security ops as one guest `dbAtomic`.
struct RpcAtomicBackend {
    /// Cap'n Proto v2 session used for a single `bookclerk.atomic` query per operation.
    session: Arc<V2PluginSession>,
    /// Full negotiated capabilities used to reject oversized plans before RPC.
    caps: DbConnectResult,
}

impl RpcAtomicBackend {
    /// Sends one `bookclerk.atomic` query; ambiguous transport maps to [`LibraryError::Unavailable`].
    async fn call(
        &self,
        params: bookclerk_library::DbAtomicParams,
    ) -> bookclerk_library::Result<bookclerk_library::DbAtomicResult> {
        let operation_id = bookclerk_library::db_atomic_operation_id(&params);
        self.call_with_id(operation_id, params).await
    }

    /// Sends `bookclerk.atomic` with a caller-chosen idempotency key (replay-safe claims).
    async fn call_with_id(
        &self,
        operation_id: String,
        params: bookclerk_library::DbAtomicParams,
    ) -> bookclerk_library::Result<bookclerk_library::DbAtomicResult> {
        let now = chrono::Utc::now().to_rfc3339();
        let compiled = bookclerk_library::compile_named_request(&operation_id, &params, &now)
            .map_err(bookclerk_library::LibraryError::Orm)?;
        self.send_compiled(compiled, operation_id).await
    }

    /// Sends an already-compiled generic plan and interprets the guest statement results.
    ///
    /// # Errors
    ///
    /// Returns when validation, `executeAtomic`, or result interpretation fails.
    async fn send_compiled(
        &self,
        compiled: bookclerk_library::CompiledAtomic,
        operation_id: String,
    ) -> bookclerk_library::Result<bookclerk_library::DbAtomicResult> {
        bookclerk_library::validate_plan(&compiled.plan, &self.caps)?;
        let mut request = compiled.clone().into_request(operation_id.clone());
        let deadline_unix_ms = unix_now_ms().saturating_add(120_000);
        request.deadline_unix_ms = Some(deadline_unix_ms);
        let typed = ExecuteRequest::from_atomic(&request)
            .map_err(|err| bookclerk_library::LibraryError::Other(anyhow::anyhow!(err)))?;
        bookclerk_library::validate_execute_request(&typed, &self.caps)?;
        let validate_req = typed.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let _guard = CancelOnDrop(Arc::clone(&cancel));
        let remaining_ms = deadline_unix_ms.saturating_sub(unix_now_ms()).max(1);
        tokio::select! {
            () = tokio::time::sleep(std::time::Duration::from_millis(remaining_ms)) => {
                cancel.store(true, Ordering::SeqCst);
                Err(bookclerk_library::LibraryError::Unavailable(
                    "deadline_exceeded: host RPC deadline elapsed".into(),
                ))
            }
            result = self.session.db_execute_request(typed, Arc::clone(&cancel)) => match result {
                Ok(reply) => {
                    bookclerk_library::validate_execute_reply(&validate_req, &reply, &self.caps)?;
                    let exec = reply.into_plan_exec();
                    bookclerk_library::validate_exec_result(
                        &compiled.plan,
                        &exec,
                        &self.caps,
                        &operation_id,
                    )?;
                    Ok(bookclerk_library::interpret_exec(
                        &compiled.plan,
                        &exec,
                        &compiled.expected_hash,
                    ))
                }
                Err(err) => Err(map_atomic_rpc_err(err)),
            }
        }
    }
}

/// Maps guest RPC failures: lost/ambiguous replies become [`LibraryError::Unavailable`].
fn map_plugin_err(err: crate::PluginError) -> bookclerk_library::LibraryError {
    if err.is_ambiguous_transport() {
        bookclerk_library::LibraryError::Unavailable(err.to_string())
    } else {
        bookclerk_library::LibraryError::Other(anyhow::anyhow!(err.to_string()))
    }
}

/// In-flight atomic abort is ambiguous (the guest may already have committed).
fn map_atomic_rpc_err(err: crate::PluginError) -> bookclerk_library::LibraryError {
    match &err {
        crate::PluginError::Abi { code, .. } if code == "cancelled" => {
            bookclerk_library::LibraryError::Unavailable(err.to_string())
        }
        _ => map_plugin_err(err),
    }
}

/// Maps host RPC failures onto the ABI [`PluginError`](AbiPluginError) the
/// granted job session returns to guests.
fn host_err_to_abi(err: crate::PluginError) -> AbiPluginError {
    match err {
        crate::PluginError::Abi { code, message } => AbiPluginError::from_wire(&code, message),
        crate::PluginError::Unavailable(message) => AbiPluginError::unavailable(message),
        other => AbiPluginError::internal(other.to_string()),
    }
}

/// Sets `cancel` when an in-flight `db_execute_request` future is dropped.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Current unix time in milliseconds.
fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Runs host DDL as one `bookclerk.atomic` batch (D1 V27).
async fn exec_host_ddl_batch(
    session: &V2PluginSession,
    caps: &DbConnectResult,
    stmts: Vec<String>,
) -> bookclerk_library::Result<()> {
    if stmts.is_empty() {
        return Ok(());
    }
    let statements: Vec<DbPlanStatement> = stmts
        .into_iter()
        .map(|sql| DbPlanStatement::new(sql, Vec::new(), DbPlanStatementKind::Execute))
        .collect();
    let plan = DbAtomicPlan {
        statements,
        outcome_index: 0,
        payload_index: None,
        prior_receipt_index: None,
        receipt_select_index: None,
    };
    let req = DbAtomicRequest {
        operation_id: format!("host-schema-{}", uuid::Uuid::new_v4()),
        request_hash: None,
        plan: Some(plan),
        deadline_unix_ms: None,
    };
    let typed = ExecuteRequest::from_atomic(&req)
        .map_err(|err| bookclerk_library::LibraryError::Other(anyhow::anyhow!(err)))?;
    let validate_req = typed.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let reply = session
        .db_execute_request(typed, cancel)
        .await
        .map_err(|err| bookclerk_library::LibraryError::Orm(DbErr::Custom(err.to_string())))?;
    bookclerk_library::validate_execute_reply(&validate_req, &reply, caps)?;
    Ok(())
}

/// Translates a guest atomic status string into a library error, or `None` on `ok`.
fn atomic_app_err(
    status: &str,
    not_found: bookclerk_library::LibraryError,
) -> Option<bookclerk_library::LibraryError> {
    match status {
        s if s == atomic_status::OK => None,
        s if s == atomic_status::NOT_FOUND => Some(not_found),
        s if s == atomic_status::LAST_OWNER => Some(bookclerk_library::LibraryError::LastOwner),
        s if s == atomic_status::CLAIM_INVALID => Some(bookclerk_library::LibraryError::Other(
            anyhow::anyhow!("claim ticket invalid, expired, or already redeemed"),
        )),
        s if s == atomic_status::PASSWORD_REQUIRED => Some(bookclerk_library::LibraryError::Other(
            anyhow::anyhow!("password required — set a password to finish claim login"),
        )),
        s if s == atomic_status::IDEMPOTENCY_CONFLICT => {
            Some(bookclerk_library::LibraryError::Other(anyhow::anyhow!(
                "database atomic operation_id reused with a different request"
            )))
        }
        other => Some(bookclerk_library::LibraryError::Other(anyhow::anyhow!(
            "database atomic operation failed: {other}"
        ))),
    }
}

/// Deserializes the atomic result payload; errors when the guest omitted it.
fn decode_payload<T: serde::de::DeserializeOwned>(
    payload: Option<Value>,
    what: &str,
) -> bookclerk_library::Result<T> {
    let value = payload.ok_or_else(|| {
        bookclerk_library::LibraryError::Other(anyhow::anyhow!(
            "database atomic operation ok without {what} payload"
        ))
    })?;
    serde_json::from_value(value).map_err(|err| {
        bookclerk_library::LibraryError::Other(anyhow::anyhow!(
            "database atomic {what} payload: {err}"
        ))
    })
}

#[async_trait]
impl bookclerk_library::TypedAtomicExec for RpcAtomicBackend {
    async fn execute_typed(
        &self,
        mut req: ExecuteRequest,
    ) -> std::result::Result<ExecuteReply, AbiPluginError> {
        bookclerk_library::authorize_typed_request(&mut req, &self.caps)
            .map_err(|err| AbiPluginError::invalid_params(err.to_string()))?;
        let validate_req = req.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let reply = self
            .session
            .db_execute_request(req, cancel)
            .await
            .map_err(host_err_to_abi)?;
        bookclerk_library::validate_execute_reply(&validate_req, &reply, &self.caps)
            .map_err(map_reply_validation_abi)?;
        Ok(reply)
    }
}

#[async_trait]
impl bookclerk_library::AtomicTxnBackend for RpcAtomicBackend {
    async fn delete_user(&self, id: i64) -> bookclerk_library::Result<()> {
        let result = self
            .call(DbAtomicParams::DeleteUser { user_id: id })
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound(format!("user {id}")),
        ) {
            return Err(err);
        }
        Ok(())
    }

    async fn set_user_status(
        &self,
        id: i64,
        status: bookclerk_library::UserStatus,
    ) -> bookclerk_library::Result<bookclerk_library::UserRecord> {
        let result = self
            .call(DbAtomicParams::SetUserStatus {
                user_id: id,
                status: status.as_str().to_string(),
            })
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound(format!("user {id}")),
        ) {
            return Err(err);
        }
        decode_payload(result.payload, "user")
    }

    async fn set_user_password_hash(
        &self,
        id: i64,
        password_hash: Option<&str>,
    ) -> bookclerk_library::Result<bookclerk_library::UserRecord> {
        let result = self
            .call(DbAtomicParams::SetUserPasswordHash {
                user_id: id,
                password_hash: password_hash.map(str::to_string),
            })
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound(format!("user {id}")),
        ) {
            return Err(err);
        }
        decode_payload(result.payload, "user")
    }

    async fn set_user_role(
        &self,
        id: i64,
        role: bookclerk_library::UserRole,
    ) -> bookclerk_library::Result<bookclerk_library::UserRecord> {
        let result = self
            .call(DbAtomicParams::SetUserRole {
                user_id: id,
                role: role.as_str().to_string(),
            })
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound(format!("user {id}")),
        ) {
            return Err(err);
        }
        decode_payload(result.payload, "user")
    }

    async fn redeem_claim_ticket_to_session(
        &self,
        token_hash: &str,
        session_hash: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
        client: Option<&bookclerk_library::SessionClientInfo>,
        new_password_hash: Option<&str>,
        password_fingerprint: Option<&str>,
    ) -> bookclerk_library::Result<bookclerk_library::PortalIdentity> {
        let result = self
            .call(DbAtomicParams::RedeemClaimTicket {
                token_hash: token_hash.to_string(),
                session_hash: session_hash.to_string(),
                expires_at: expires_at.to_rfc3339(),
                user_agent: client.and_then(|c| c.user_agent.clone()),
                device_type: client.map(|c| c.device_type.clone()),
                client_label: client.map(|c| c.client_label.clone()),
                new_password_hash: new_password_hash.map(str::to_string),
                password_fingerprint: password_fingerprint.map(str::to_string),
            })
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::Other(anyhow::anyhow!("claim ticket not found")),
        ) {
            return Err(err);
        }
        decode_payload(result.payload, "portal identity")
    }

    async fn take_oidc_rp_state(
        &self,
        state_hash: &str,
    ) -> bookclerk_library::Result<Option<(String, String, String, String, Option<i64>)>> {
        let result = self
            .call(DbAtomicParams::TakeOidcRpState {
                state_hash: state_hash.to_string(),
            })
            .await?;
        if result.status == atomic_status::EMPTY {
            return Ok(None);
        }
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound("oidc rp state".into()),
        ) {
            return Err(err);
        }
        let row: AtomicOidcRpState = decode_payload(result.payload, "oidc rp state")?;
        Ok(Some((
            row.provider_id,
            row.pkce_verifier,
            row.nonce,
            row.purpose,
            row.user_id,
        )))
    }

    async fn take_webauthn_challenge(
        &self,
        challenge_id: &str,
        kind: &str,
    ) -> bookclerk_library::Result<Option<(Option<i64>, String)>> {
        let result = self
            .call(DbAtomicParams::TakeWebauthnChallenge {
                challenge_id: challenge_id.to_string(),
                kind: kind.to_string(),
            })
            .await?;
        if result.status == atomic_status::EMPTY {
            return Ok(None);
        }
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound("webauthn challenge".into()),
        ) {
            return Err(err);
        }
        let row: AtomicWebauthnChallenge = decode_payload(result.payload, "webauthn challenge")?;
        Ok(Some((row.user_id, row.state_json)))
    }

    async fn enqueue_job(
        &self,
        spec: bookclerk_library::EnqueueJobSpec,
    ) -> bookclerk_library::Result<bookclerk_library::EnqueueOutcome> {
        let payload_json = serde_json::to_string(&spec.payload).map_err(|err| {
            bookclerk_library::LibraryError::Other(anyhow::anyhow!(err.to_string()))
        })?;
        let result = self
            .call(DbAtomicParams::EnqueueJob {
                kind: spec.kind.as_str().to_string(),
                payload_json,
                priority: spec.priority,
                max_attempts: spec.max_attempts,
                max_pending: spec.max_pending,
                run_after: spec.run_after.map(|t| t.to_rfc3339()),
            })
            .await?;
        match result.status.as_str() {
            s if s == atomic_status::OK => {
                let id = result
                    .payload
                    .as_ref()
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        bookclerk_library::LibraryError::Other(anyhow::anyhow!(
                            "enqueueJob ok without id"
                        ))
                    })?
                    .to_string();
                Ok(bookclerk_library::EnqueueOutcome::Created { id })
            }
            s if s == atomic_status::DUPLICATE => {
                let existing_id = result
                    .payload
                    .as_ref()
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        bookclerk_library::LibraryError::Other(anyhow::anyhow!(
                            "enqueueJob duplicate without id"
                        ))
                    })?
                    .to_string();
                Ok(bookclerk_library::EnqueueOutcome::Duplicate { existing_id })
            }
            s if s == atomic_status::QUEUE_FULL => Ok(bookclerk_library::EnqueueOutcome::QueueFull),
            other => Err(bookclerk_library::LibraryError::Other(anyhow::anyhow!(
                "enqueueJob failed: {other}"
            ))),
        }
    }

    async fn claim_next_job(
        &self,
        resource_class: bookclerk_library::JobResourceClass,
        owner: &str,
        lease_secs: u64,
        operation_id: &str,
    ) -> bookclerk_library::Result<Option<bookclerk_library::JobRecord>> {
        let result = self
            .call_with_id(
                operation_id.to_string(),
                DbAtomicParams::ClaimNextJob {
                    resource_class: resource_class.as_str().to_string(),
                    owner: owner.to_string(),
                    lease_secs: i64::try_from(lease_secs).unwrap_or(60),
                },
            )
            .await?;
        if result.status == atomic_status::EMPTY {
            return Ok(None);
        }
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound("job".into()),
        ) {
            return Err(err);
        }
        decode_payload(result.payload, "job")
    }

    async fn reserve_job_temp_path(
        &self,
        job_id: &str,
        path: &str,
        reserved_bytes: u64,
        quota_bytes: u64,
    ) -> bookclerk_library::Result<()> {
        let result = self
            .call(DbAtomicParams::ReserveJobTemp {
                job_id: job_id.to_string(),
                path: path.to_string(),
                reserved_bytes: i64::try_from(reserved_bytes).unwrap_or(i64::MAX),
                quota_bytes: i64::try_from(quota_bytes).unwrap_or(i64::MAX),
            })
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::Other(anyhow::anyhow!(
                "acquire scratch quota exceeded"
            )),
        ) {
            return Err(err);
        }
        Ok(())
    }

    async fn confirm_totp_enrollment(
        &self,
        user_id: i64,
        record: &bookclerk_library::EncryptedSecretRecord,
    ) -> bookclerk_library::Result<()> {
        let result = self
            .call(DbAtomicParams::ConfirmTotpEnrollment {
                user_id,
                format: record.format.clone(),
                ciphertext: bookclerk_library::bytes_to_b64_string(&record.ciphertext),
                cipher_algorithm: record.cipher_algorithm.clone(),
                cipher_nonce: record
                    .cipher_nonce
                    .as_deref()
                    .map(bookclerk_library::bytes_to_b64_string),
                kdf_algorithm: record.kdf_algorithm.clone(),
                kdf_salt: record
                    .kdf_salt
                    .as_deref()
                    .map(bookclerk_library::bytes_to_b64_string),
                kdf_m_cost: record.kdf_m_cost.map(i64::from),
                kdf_t_cost: record.kdf_t_cost.map(i64::from),
                kdf_p_cost: record.kdf_p_cost.map(i64::from),
                created_at: record.created_at.clone(),
            })
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound("user".into()),
        ) {
            return Err(err);
        }
        Ok(())
    }

    async fn disable_user_totp(&self, user_id: i64) -> bookclerk_library::Result<()> {
        let result = self
            .call(DbAtomicParams::DisableUserTotp { user_id })
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound("user".into()),
        ) {
            return Err(err);
        }
        Ok(())
    }

    async fn publish_domain_event(
        &self,
        spec: bookclerk_library::PublishDomainEventSpec,
    ) -> bookclerk_library::Result<bookclerk_library::PublishDomainEventOutcome> {
        let spec = bookclerk_library::prepare_publish_domain_event(spec)?;
        let result = self
            .call(DbAtomicParams::PublishDomainEvent {
                id: spec.id,
                event_type: spec.event_type,
                schema_version: spec.schema_version,
                account_id: spec.account_id,
                source: spec.source,
                correlation_id: spec.correlation_id,
                causation_id: spec.causation_id,
                dedup_key: spec.dedup_key,
                payload: spec.payload,
                ordering_key: spec.ordering_key,
            })
            .await?;
        match result.status.as_str() {
            s if s == atomic_status::OK => {
                let id = result
                    .payload
                    .as_ref()
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        bookclerk_library::LibraryError::Other(anyhow::anyhow!(
                            "publishDomainEvent ok without id"
                        ))
                    })?
                    .to_string();
                Ok(bookclerk_library::PublishDomainEventOutcome::Created { id })
            }
            s if s == atomic_status::DUPLICATE => {
                let existing_id = result
                    .payload
                    .as_ref()
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        bookclerk_library::LibraryError::Other(anyhow::anyhow!(
                            "publishDomainEvent duplicate without id"
                        ))
                    })?
                    .to_string();
                Ok(bookclerk_library::PublishDomainEventOutcome::Duplicate { existing_id })
            }
            other => Err(bookclerk_library::LibraryError::Other(anyhow::anyhow!(
                "publishDomainEvent failed: {other}"
            ))),
        }
    }

    async fn dispatch_event_deliveries(
        &self,
        event_id: &str,
        subscribers: &[bookclerk_library::EventSubscriber],
        operation_id: &str,
        mark_dispatched: bool,
    ) -> bookclerk_library::Result<u32> {
        let subscribers_json = serde_json::to_string(subscribers).map_err(|err| {
            bookclerk_library::LibraryError::Other(anyhow::anyhow!(err.to_string()))
        })?;
        let result = self
            .call_with_id(
                operation_id.to_string(),
                DbAtomicParams::DispatchEventDeliveries {
                    event_id: event_id.to_string(),
                    subscribers_json,
                    mark_dispatched,
                },
            )
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound(format!("event {event_id}")),
        ) {
            return Err(err);
        }
        Ok(result
            .payload
            .as_ref()
            .and_then(|v| v.get("created"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32)
    }

    #[allow(clippy::too_many_arguments)]
    async fn claim_event_delivery(
        &self,
        delivery_id: &str,
        owner: &str,
        lease_secs: u64,
        operation_id: &str,
        plugin_id: &str,
        resource_class: &str,
        max_in_flight: u32,
    ) -> bookclerk_library::Result<Option<bookclerk_library::EventDeliveryRecord>> {
        let now = chrono::Utc::now().to_rfc3339();
        let compiled = bookclerk_library::compile_claim_event_delivery(
            operation_id,
            delivery_id,
            owner,
            i64::try_from(lease_secs).unwrap_or(60),
            plugin_id,
            resource_class,
            i64::from(max_in_flight),
            &now,
        )
        .map_err(bookclerk_library::LibraryError::Orm)?;
        let result = self
            .send_compiled(compiled, operation_id.to_string())
            .await?;
        if result.status == atomic_status::EMPTY {
            return Ok(None);
        }
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound("event delivery".into()),
        ) {
            return Err(err);
        }
        decode_payload(result.payload, "event delivery").map(Some)
    }

    async fn set_acquire_status(
        &self,
        book_uuid: &str,
        status: bookclerk_library::AcquireStatus,
        storage_key: Option<&str>,
        error_message: Option<&str>,
        event: Option<bookclerk_library::PublishDomainEventSpec>,
    ) -> bookclerk_library::Result<()> {
        let event = match event {
            Some(spec) => bookclerk_library::prepare_publish_domain_event(spec)?,
            None => bookclerk_library::PublishDomainEventSpec {
                id: String::new(),
                event_type: String::new(),
                schema_version: 0,
                account_id: String::new(),
                source: String::new(),
                correlation_id: String::new(),
                causation_id: String::new(),
                dedup_key: String::new(),
                payload: String::new(),
                ordering_key: String::new(),
            },
        };
        let result = self
            .call(DbAtomicParams::SetAcquireStatus {
                book_uuid: book_uuid.to_string(),
                status: status.as_str().to_string(),
                storage_key: storage_key.map(str::to_string),
                error_message: error_message.map(str::to_string),
                event_id: event.id,
                event_type: event.event_type,
                schema_version: event.schema_version,
                event_account_id: event.account_id,
                source: event.source,
                correlation_id: event.correlation_id,
                causation_id: event.causation_id,
                dedup_key: event.dedup_key,
                payload: event.payload,
                ordering_key: event.ordering_key,
            })
            .await?;
        if let Some(err) = atomic_app_err(
            &result.status,
            bookclerk_library::LibraryError::NotFound(format!("book {book_uuid}")),
        ) {
            return Err(err);
        }
        Ok(())
    }
}

#[derive(serde::Deserialize)]
/// One-shot OIDC RP state returned by guest `takeOidcRpState` (consumed on read).
struct AtomicOidcRpState {
    /// Configured OIDC provider id that minted this state.
    provider_id: String,
    /// PKCE code verifier for the pending authorization-code exchange.
    pkce_verifier: String,
    /// Nonce that must match the verified ID token.
    nonce: String,
    /// Login vs link (or other) purpose recorded when the state was stored.
    purpose: String,
    #[serde(default)]
    /// Existing user to link when this is not a JIT signup, if any.
    user_id: Option<i64>,
}

#[derive(serde::Deserialize)]
/// One-shot WebAuthn challenge returned by guest `takeWebauthnChallenge`.
struct AtomicWebauthnChallenge {
    #[serde(default)]
    /// User this challenge was issued for; `None` during usernameless login.
    user_id: Option<i64>,
    /// Opaque webauthn-rs state JSON needed to finish the ceremony.
    state_json: String,
}

/// Builds guest `db.connect` params (SQLite path, D1 token, or Postgres URL) from host config.
fn connect_params(
    config: &Config,
    plugin_id: &str,
    plugin_data_dir: &Path,
    session: &V2PluginSession,
) -> Result<DbConnectParams, DbErr> {
    let data_dir = plugin_data_dir.display().to_string();
    match DatabasePluginKind::parse(plugin_id) {
        Some(DatabasePluginKind::Sqlite) => Ok(sqlite_connect_params(config, plugin_data_dir)),
        Some(DatabasePluginKind::D1) => {
            session
                .require_binding("secrets")
                .map_err(|err| DbErr::Custom(err.to_string()))?;
            Ok(DbConnectParams::D1 {
                plugin_data_dir: data_dir,
                account_id: config.database.d1.account_id.clone(),
                database_id: config.database.d1.database_id.clone(),
                api_base: config.database.d1.api_base.clone(),
                api_token: resolve_d1_api_token().map_err(map_config_err)?,
            })
        }
        Some(DatabasePluginKind::Postgres) => {
            session
                .require_binding("secrets")
                .map_err(|err| DbErr::Custom(err.to_string()))?;
            Ok(DbConnectParams::Postgres {
                plugin_data_dir: data_dir,
                url: resolve_postgres_url(config).map_err(map_config_err)?,
            })
        }
        None => Ok(sqlite_connect_params(config, plugin_data_dir)),
    }
}

/// Maps advertised schema capabilities to a SeaORM [`DbBackend`].
///
/// Schema kind is the versioning mechanic, not SQL-family identity. Prefer
/// [`seaorm_backend_from_connect`] when opening the SeaORM proxy.
#[cfg(test)]
fn schema_kind_to_backend(kind: bookclerk_library::HostSchemaKind) -> DbBackend {
    match kind {
        bookclerk_library::HostSchemaKind::PragmaMarker
        | bookclerk_library::HostSchemaKind::AtomicBatchMarker => DbBackend::Sqlite,
        bookclerk_library::HostSchemaKind::RowMarker => DbBackend::Postgres,
    }
}

/// SeaORM proxy backend from bootstrap metadata on a connect result.
fn seaorm_backend_from_connect(connect: &DbConnectResult) -> Result<DbBackend, DbErr> {
    match connect.sql_family.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" | "pg" => return Ok(DbBackend::Postgres),
        "sqlite" => return Ok(DbBackend::Sqlite),
        "" => {}
        other => {
            return Err(DbErr::Custom(format!(
                "unknown database bootstrap sqlFamily `{other}`"
            )));
        }
    }
    match connect.dialect.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" | "pg" => Ok(DbBackend::Postgres),
        "sqlite" => Ok(DbBackend::Sqlite),
        other => Err(DbErr::Custom(format!(
            "unknown database bootstrap dialect `{other}`"
        ))),
    }
}

/// SQLite `db.connect` params for first-party `sqlite` and arbitrary sqlite-family ids.
fn sqlite_connect_params(config: &Config, plugin_data_dir: &Path) -> DbConnectParams {
    let path = config.database.sqlite_path(&config.paths().files_dir);
    DbConnectParams::Sqlite {
        plugin_data_dir: plugin_data_dir.display().to_string(),
        sqlite_path: Some(path.display().to_string()),
    }
}

/// Maps host reply validation onto a plugin-host RPC error.
fn map_reply_validation_err(err: bookclerk_library::LibraryError) -> crate::PluginError {
    match err {
        bookclerk_library::LibraryError::Unavailable(message) => {
            crate::PluginError::Unavailable(message)
        }
        other => crate::PluginError::from_abi(Some("invalid_params"), other.to_string()),
    }
}

/// Maps host reply validation onto the ABI error returned to guests.
fn map_reply_validation_abi(err: bookclerk_library::LibraryError) -> AbiPluginError {
    match err {
        bookclerk_library::LibraryError::Unavailable(message) => {
            AbiPluginError::unavailable(message)
        }
        other => AbiPluginError::invalid_params(other.to_string()),
    }
}

/// Wraps a plugin RPC failure as a SeaORM [`DbErr::Custom`].
fn map_rpc_err(err: crate::PluginError) -> DbErr {
    DbErr::Custom(err.to_string())
}

/// Wraps a host config error (missing D1 token / Postgres URL) as [`DbErr::Custom`].
fn map_config_err(err: bookclerk_config::ConfigError) -> DbErr {
    DbErr::Custom(err.to_string())
}

/// Converts plugin settings TOML to JSON for guest spawn; invalid values become `null`.
fn toml_to_json(value: &toml::Value) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_schema_kind_to_backend() {
        assert_eq!(
            schema_kind_to_backend(bookclerk_library::HostSchemaKind::PragmaMarker),
            DbBackend::Sqlite
        );
        assert_eq!(
            schema_kind_to_backend(bookclerk_library::HostSchemaKind::AtomicBatchMarker),
            DbBackend::Sqlite
        );
        assert_eq!(
            schema_kind_to_backend(bookclerk_library::HostSchemaKind::RowMarker),
            DbBackend::Postgres
        );
        let kind = bookclerk_library::HostSchemaKind::from_capabilities(&DbConnectResult::sqlite())
            .unwrap();
        assert_eq!(kind, bookclerk_library::HostSchemaKind::PragmaMarker);
        assert_eq!(schema_kind_to_backend(kind), DbBackend::Sqlite);
    }

    #[test]
    fn sqlite_row_migrations_family_is_sqlite_kind_is_row_marker() {
        let caps = DbConnectResult::sqlite_row_migrations();
        let kind = bookclerk_library::HostSchemaKind::from_capabilities(&caps).unwrap();
        assert_eq!(kind, bookclerk_library::HostSchemaKind::RowMarker);
        assert_eq!(
            seaorm_backend_from_connect(&caps).unwrap(),
            DbBackend::Sqlite
        );
    }

    #[test]
    fn unknown_plugin_id_uses_sqlite_connect_params() {
        assert!(DatabasePluginKind::parse("sql-conformance").is_none());
        let files = tempfile::tempdir().expect("tempdir");
        let config = Config {
            paths: Some(bookclerk_config::Paths::from_files_dir(
                files.path().to_path_buf(),
            )),
            ..Config::default()
        };
        let params = sqlite_connect_params(&config, Path::new("/tmp/plugin-data"));
        match params {
            DbConnectParams::Sqlite {
                sqlite_path,
                plugin_data_dir,
            } => {
                assert!(sqlite_path.is_some(), "{sqlite_path:?}");
                assert_eq!(plugin_data_dir, "/tmp/plugin-data");
            }
            other => panic!("expected sqlite params, got {other:?}"),
        }
    }

    #[test]
    fn ambiguous_plugin_errors_map_to_unavailable() {
        let err = crate::PluginError::unavailable("D1 HTTP 502");
        assert!(err.is_ambiguous_transport());
        assert!(matches!(
            map_plugin_err(err),
            bookclerk_library::LibraryError::Unavailable(_)
        ));
        let other = crate::PluginError::message("no such table");
        assert!(!other.is_ambiguous_transport());
        assert!(matches!(
            map_plugin_err(other),
            bookclerk_library::LibraryError::Other(_)
        ));
        let permanent = crate::PluginError::message("D1 HTTP 400: SQL error");
        assert!(!permanent.is_ambiguous_transport());
    }

    #[test]
    fn consume_once_rpc_payload_reuses_stable_operation_id() {
        let params = DbAtomicParams::TakeOidcRpState {
            state_hash: "abc".into(),
        };
        let first = bookclerk_library::db_atomic_operation_id(&params);
        let second = bookclerk_library::db_atomic_operation_id(&params);
        assert_eq!(first, second);
        assert_eq!(first, "takeOidcRpState:abc");
    }

    #[test]
    fn omitted_rows_affected_fails_deserialize() {
        let err = serde_json::from_str::<bookclerk_plugin_sdk::legacy_db::DbPlanStmtExecResult>(
            r#"{"rows":[]}"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("rowsAffected") || err.to_string().contains("missing field"),
            "{err}"
        );
    }

    #[test]
    fn malformed_atomic_json_maps_to_unavailable() {
        let err = crate::PluginError::unavailable("lost reply");
        assert!(matches!(
            map_plugin_err(err),
            bookclerk_library::LibraryError::Unavailable(_)
        ));
    }

    #[tokio::test]
    async fn granted_job_database_allows_books_and_denies_jobs() {
        let store = bookclerk_library::LibraryStore::from_connection(
            bookclerk_plugin_database_sqlite::open_memory()
                .await
                .unwrap(),
        )
        .with_connect_result(DbConnectResult::sqlite());
        let session = granted_job_database(store);
        session
            .execute(ExecuteRequest {
                operation_id: "job-books".into(),
                request_hash: String::new(),
                statements: vec![TypedDbStatement {
                    sql: "SELECT id FROM books LIMIT 1".into(),
                    parameters: vec![],
                    kind: DbPlanStatementKind::Select,
                    max_rows: 8,
                    result_selection: DbResultSelection::Rows,
                }],
                deadline_unix_ms: 0,
            })
            .await
            .expect("books grant");
        let err = session
            .execute(ExecuteRequest {
                operation_id: "job-jobs".into(),
                request_hash: String::new(),
                statements: vec![TypedDbStatement {
                    sql: "SELECT id FROM jobs".into(),
                    parameters: vec![],
                    kind: DbPlanStatementKind::Select,
                    max_rows: 8,
                    result_selection: DbResultSelection::Rows,
                }],
                deadline_unix_ms: 0,
            })
            .await
            .expect_err("jobs denied");
        assert_eq!(
            err.code,
            bookclerk_plugin_sdk::PluginErrorCode::InvalidParams
        );
        assert!(err.to_string().contains("unauthorized table"), "{err}");
    }
    #[tokio::test]
    async fn granted_job_database_binding_retries_and_conflicts_on_counters() {
        use bookclerk_plugin_sdk::{DatabaseBinding, DbValue, RetryToken};
        use sea_orm::{ConnectionTrait, Statement};

        let store = bookclerk_library::LibraryStore::from_connection(
            bookclerk_plugin_database_sqlite::open_memory()
                .await
                .unwrap(),
        )
        .with_connect_result(DbConnectResult::sqlite());
        store
            .connection()
            .execute_raw(Statement::from_string(
                DbBackend::Sqlite,
                "CREATE TABLE counters (id INTEGER PRIMARY KEY, n INTEGER NOT NULL DEFAULT 0)",
            ))
            .await
            .unwrap();
        let session = granted_job_database_with_policy(
            store,
            bookclerk_library::GuestSqlPolicy::allow_tables(["counters"]),
        );
        let binding = DatabaseBinding::from_session(session);
        let token = RetryToken {
            operation_id: "inc-1".into(),
            request_hash: String::new(),
        };
        let insert = "INSERT INTO counters (id, n) VALUES (?, ?)";
        binding
            .prepare(insert)
            .bind(vec![DbValue::Int64(1), DbValue::Int64(1)])
            .run(Some(token.clone()))
            .await
            .expect("first insert");
        binding
            .prepare(insert)
            .bind(vec![DbValue::Int64(1), DbValue::Int64(1)])
            .run(Some(token.clone()))
            .await
            .expect("idempotent retry");
        let row = binding
            .prepare("SELECT n FROM counters WHERE id = 1")
            .first(None)
            .await
            .expect("select")
            .expect("row");
        let n = row
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("n"))
            .map(|(_, v)| v.clone())
            .expect("n column");
        assert_eq!(n, DbValue::Int64(1), "{n:?}");
        let err = binding
            .prepare(insert)
            .bind(vec![DbValue::Int64(1), DbValue::Int64(99)])
            .run(Some(token))
            .await
            .expect_err("hash mismatch must conflict");
        assert_eq!(err.code, bookclerk_plugin_sdk::PluginErrorCode::Conflict);
        let row = binding
            .prepare("SELECT n FROM counters WHERE id = 1")
            .first(None)
            .await
            .expect("select after conflict")
            .expect("row");
        let n = row
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("n"))
            .map(|(_, v)| v.clone())
            .expect("n column");
        assert_eq!(n, DbValue::Int64(1), "{n:?}");
    }
}
