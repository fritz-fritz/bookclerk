//! Branded [`BookclerkPlugin`] guest trait.
//!
//! Audience: authors of **native** (`runtime = "native"`) guests. Implement
//! [`BookclerkPlugin`] for the methods your plugin kind needs, then wrap with
//! [`crate::V2PluginRoot`] and call [`crate::serve`] / [`crate::serve_v2`].
//! Workerd guests reuse the same method surface via [`crate::workerd`] / the
//! npm package.
//!
//! Wire names are camelCase Workers RPC methods (see `bookclerk-plugin-abi` and
//! [`crate::protocol::methods`]). Defaults return unsupported errors so unused
//! methods stay off the capability surface.

use async_trait::async_trait;
use serde_json::Value;

use bookclerk_plugin_abi::{
    AuthenticateUserParams, CatalogDetailParams, CatalogHitDto, CliInvokeParams, CliInvokeResult,
    CliSchema, CredentialsUpdateParams, DbAtomicRequest, DbAtomicResult, DbBeginParams,
    DbBeginResult, DbConnectParams, DbConnectResult, DbTxnParams, DiagnoseResult,
    EventPollResultDto, ExecResultDto, ExistsResultDto, ExpandCandidatesParams, ExternalUserDto,
    FetchTitleParams, GetResultDto, HandshakeParams, HandshakeResult, HealthResult,
    HostToPluginEvent, ListAccountsParams, ListDealsParams, LoginCompleteParams, LoginParams,
    LoginResultDto, LoginStartParams, LoginStartResultDto, ObjectInfoDto, ObjectProbeDto,
    OutputCopyParams, OutputGetParams, OutputKeyParams, OutputListParams, OutputPutFileParams,
    OutputPutParams, OutputTouchFileParams, PluginError, PluginErrorCode, PurchaseHintDto,
    PurchaseHintParams, QueryResultDto, ScanLibraryParams, ScanParams, ScanSummaryDto,
    SearchCatalogParams, SourceAccountDto, SourceFetchDto, StatementDto, SyncListeningResultDto,
};

/// Branded guest contract — identical method surface for native and workerd SDKs.
///
/// Implement the methods your plugin kind needs; defaults return
/// [`PluginError::unsupported`]. Wire names are camelCase Workers RPC methods
/// (see `bookclerk-plugin-abi`).
#[async_trait]
pub trait BookclerkPlugin: Send + Sync + 'static {
    /// Negotiates ABI version and advertises plugin identity to the host.
    ///
    /// # Arguments
    ///
    /// * `params` - Host-provided handshake inputs (API version, install paths).
    ///
    /// # Returns
    ///
    /// Guest identity, kind, and negotiated capabilities.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError`] when the guest rejects the host ABI version.
    async fn handshake(
        &self,
        params: HandshakeParams,
    ) -> std::result::Result<HandshakeResult, PluginError>;

    /// Releases guest resources before the process exits.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError`] when cleanup fails.
    async fn shutdown(&self) -> std::result::Result<(), PluginError> {
        Ok(())
    }

    /// Reports liveness for host health checks.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError`] when the guest cannot evaluate health.
    async fn health(&self) -> std::result::Result<HealthResult, PluginError> {
        Ok(HealthResult {
            ok: true,
            ..HealthResult::default()
        })
    }

    /// Collects operator-facing diagnostic lines for `plugins doctor`.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError`] when diagnostics cannot be gathered.
    async fn diagnose(&self) -> std::result::Result<DiagnoseResult, PluginError> {
        Ok(DiagnoseResult { lines: vec![] })
    }

    /// Starts long-running guest work after a successful handshake.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn start(&self) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("start not implemented"))
    }

    /// Handles a host-to-plugin push event.
    ///
    /// # Arguments
    ///
    /// * `_event` - Event envelope from the host.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn on_event(&self, _event: HostToPluginEvent) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("onEvent not implemented"))
    }

    /// Drains queued plugin-to-host events.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn poll_events(&self) -> std::result::Result<EventPollResultDto, PluginError> {
        Err(PluginError::unsupported("pollEvents not implemented"))
    }

    /// Scans the operator library through an integration guest.
    ///
    /// # Arguments
    ///
    /// * `_params` - Scan scope and account filters.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn scan_library(
        &self,
        _params: ScanLibraryParams,
    ) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("scanLibrary not implemented"))
    }

    /// Syncs listening progress with an external library server.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn sync_listening(&self) -> std::result::Result<SyncListeningResultDto, PluginError> {
        Err(PluginError::unsupported("syncListening not implemented"))
    }

    /// Validates an external user identity for portal / OIDC flows.
    ///
    /// # Arguments
    ///
    /// * `_params` - Credentials or tokens from the connect portal.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn authenticate_user(
        &self,
        _params: AuthenticateUserParams,
    ) -> std::result::Result<ExternalUserDto, PluginError> {
        Err(PluginError::unsupported("authenticateUser not implemented"))
    }

    /// Describes guest CLI commands for `bookclerk plugin` / host plumbing.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError`] when the schema cannot be built.
    async fn cli_describe(&self) -> std::result::Result<CliSchema, PluginError> {
        Ok(CliSchema::default())
    }

    /// Invokes a guest CLI command described by [`Self::cli_describe`].
    ///
    /// # Arguments
    ///
    /// * `_params` - Command name and argv-style arguments.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn cli_invoke(
        &self,
        _params: CliInvokeParams,
    ) -> std::result::Result<CliInvokeResult, PluginError> {
        Err(PluginError::unsupported("cliInvoke not implemented"))
    }

    /// Performs a one-shot store login (password / token flows).
    ///
    /// # Arguments
    ///
    /// * `_params` - Store credentials and account labeling.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn login(
        &self,
        _params: LoginParams,
    ) -> std::result::Result<LoginResultDto, PluginError> {
        Err(PluginError::unsupported("login not implemented"))
    }

    /// Starts an interactive OAuth (or similar) login.
    ///
    /// # Arguments
    ///
    /// * `_params` - Marketplace / locale hints for the authorize URL.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn login_start(
        &self,
        _params: LoginStartParams,
    ) -> std::result::Result<LoginStartResultDto, PluginError> {
        Err(PluginError::unsupported("loginStart not implemented"))
    }

    /// Completes an interactive login started by [`Self::login_start`].
    ///
    /// # Arguments
    ///
    /// * `_params` - Callback payload / authorization code.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn login_complete(
        &self,
        _params: LoginCompleteParams,
    ) -> std::result::Result<LoginResultDto, PluginError> {
        Err(PluginError::unsupported("loginComplete not implemented"))
    }

    /// Updates stored credentials without a full login round-trip.
    ///
    /// # Arguments
    ///
    /// * `_params` - Account id and replacement secret material.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn credentials_update(
        &self,
        _params: CredentialsUpdateParams,
    ) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported(
            "credentialsUpdate not implemented",
        ))
    }

    /// Scans owned titles from a source storefront.
    ///
    /// # Arguments
    ///
    /// * `_params` - Account filters and pagination options.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn scan(&self, _params: ScanParams) -> std::result::Result<ScanSummaryDto, PluginError> {
        Err(PluginError::unsupported("scan not implemented"))
    }

    /// Downloads (and decrypts, when applicable) one title to a fetch directory.
    ///
    /// # Arguments
    ///
    /// * `_params` - Title id / ASIN / ISBN and destination hints.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn fetch_title(
        &self,
        _params: FetchTitleParams,
    ) -> std::result::Result<SourceFetchDto, PluginError> {
        Err(PluginError::unsupported("fetchTitle not implemented"))
    }

    /// Searches the storefront catalog.
    ///
    /// # Arguments
    ///
    /// * `_params` - Query string and optional filters.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn search_catalog(
        &self,
        _params: SearchCatalogParams,
    ) -> std::result::Result<Vec<CatalogHitDto>, PluginError> {
        Err(PluginError::unsupported("searchCatalog not implemented"))
    }

    /// Expands related catalog candidates for a seed title.
    ///
    /// # Arguments
    ///
    /// * `_params` - Seed identifiers and expansion limits.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn expand_candidates(
        &self,
        _params: ExpandCandidatesParams,
    ) -> std::result::Result<Vec<CatalogHitDto>, PluginError> {
        Err(PluginError::unsupported("expandCandidates not implemented"))
    }

    /// Returns a purchase / wishlist hint for a catalog title when available.
    ///
    /// # Arguments
    ///
    /// * `_params` - Title identifier in the storefront namespace.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn purchase_hint(
        &self,
        _params: PurchaseHintParams,
    ) -> std::result::Result<Option<PurchaseHintDto>, PluginError> {
        Err(PluginError::unsupported("purchaseHint not implemented"))
    }

    /// Lists current deals / sales from the storefront.
    ///
    /// # Arguments
    ///
    /// * `_params` - Pagination and marketplace filters.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn list_deals(
        &self,
        _params: ListDealsParams,
    ) -> std::result::Result<Vec<CatalogHitDto>, PluginError> {
        Err(PluginError::unsupported("listDeals not implemented"))
    }

    /// Lists connected source accounts known to this guest.
    ///
    /// # Arguments
    ///
    /// * `_params` - Optional account-id filter.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn list_accounts(
        &self,
        _params: ListAccountsParams,
    ) -> std::result::Result<Vec<SourceAccountDto>, PluginError> {
        Err(PluginError::unsupported("listAccounts not implemented"))
    }

    /// Fetches rich catalog detail for one title.
    ///
    /// # Arguments
    ///
    /// * `_params` - Storefront title identifier.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn catalog_detail(
        &self,
        _params: CatalogDetailParams,
    ) -> std::result::Result<Option<CatalogHitDto>, PluginError> {
        Err(PluginError::unsupported("catalogDetail not implemented"))
    }

    /// Writes bytes to a destination object key.
    ///
    /// # Arguments
    ///
    /// * `_params` - Object key and inline payload.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn put(&self, _params: OutputPutParams) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("put not implemented"))
    }

    /// Uploads a local file to a destination object key.
    ///
    /// # Arguments
    ///
    /// * `_params` - Object key and absolute source path.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn put_file(&self, _params: OutputPutFileParams) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("putFile not implemented"))
    }

    /// Reads an object from the destination.
    ///
    /// # Arguments
    ///
    /// * `_params` - Object key and optional byte range.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn get(
        &self,
        _params: OutputGetParams,
    ) -> std::result::Result<GetResultDto, PluginError> {
        Err(PluginError::unsupported("get not implemented"))
    }

    /// Checks whether an object key exists.
    ///
    /// # Arguments
    ///
    /// * `_params` - Object key.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn exists(
        &self,
        _params: OutputKeyParams,
    ) -> std::result::Result<ExistsResultDto, PluginError> {
        Err(PluginError::unsupported("exists not implemented"))
    }

    /// Lists objects under a destination prefix.
    ///
    /// # Arguments
    ///
    /// * `_params` - Prefix and pagination cursor.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn list(
        &self,
        _params: OutputListParams,
    ) -> std::result::Result<Vec<ObjectInfoDto>, PluginError> {
        Err(PluginError::unsupported("list not implemented"))
    }

    /// Probes object metadata without downloading the body.
    ///
    /// # Arguments
    ///
    /// * `_params` - Object key.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn probe(
        &self,
        _params: OutputKeyParams,
    ) -> std::result::Result<ObjectProbeDto, PluginError> {
        Err(PluginError::unsupported("probe not implemented"))
    }

    /// Copies an object from one key to another inside the destination.
    ///
    /// # Arguments
    ///
    /// * `_params` - Source and destination keys.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn copy(&self, _params: OutputCopyParams) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("copy not implemented"))
    }

    /// Deletes an object key from the destination.
    ///
    /// # Arguments
    ///
    /// * `_params` - Object key.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn delete(&self, _params: OutputKeyParams) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("delete not implemented"))
    }

    /// Updates timestamps / metadata on an existing local destination file.
    ///
    /// # Arguments
    ///
    /// * `_params` - Absolute path and touch options.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn touch_file(
        &self,
        _params: OutputTouchFileParams,
    ) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("touchFile not implemented"))
    }

    /// Opens a database guest connection (SeaORM proxy / D1 / Postgres).
    ///
    /// # Arguments
    ///
    /// * `_params` - Backend-specific connect parameters.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn db_connect(
        &self,
        _params: DbConnectParams,
    ) -> std::result::Result<DbConnectResult, PluginError> {
        Err(PluginError::unsupported("dbConnect not implemented"))
    }

    /// Pings an open database guest connection.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn db_ping(&self) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("dbPing not implemented"))
    }

    /// Runs a read query through the database guest.
    ///
    /// # Arguments
    ///
    /// * `_params` - SQL statement and bound values.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn db_query(
        &self,
        _params: StatementDto,
    ) -> std::result::Result<QueryResultDto, PluginError> {
        Err(PluginError::unsupported("dbQuery not implemented"))
    }

    /// Runs a write statement through the database guest.
    ///
    /// # Arguments
    ///
    /// * `_params` - SQL statement and bound values.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn db_execute(
        &self,
        _params: StatementDto,
    ) -> std::result::Result<ExecResultDto, PluginError> {
        Err(PluginError::unsupported("dbExecute not implemented"))
    }

    /// Begins a database transaction (or nested savepoint).
    ///
    /// # Arguments
    ///
    /// * `_params` - Optional parent transaction id for nested savepoints.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn db_begin(
        &self,
        _params: DbBeginParams,
    ) -> std::result::Result<DbBeginResult, PluginError> {
        Err(PluginError::unsupported("dbBegin not implemented"))
    }

    /// Commits a guest transaction returned by [`Self::db_begin`].
    ///
    /// # Arguments
    ///
    /// * `_params` - Transaction id to commit.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn db_commit(&self, _params: DbTxnParams) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("dbCommit not implemented"))
    }

    /// Rolls back a guest transaction returned by [`Self::db_begin`].
    ///
    /// # Arguments
    ///
    /// * `_params` - Transaction id to roll back.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn db_rollback(&self, _params: DbTxnParams) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("dbRollback not implemented"))
    }

    /// Runs a named atomic library operation as one guest SQL transaction.
    ///
    /// Bundled database guests implement this. D1 uses one HTTP `batch()`;
    /// SQLite and Postgres use a native local transaction. Both persist a
    /// receipt keyed by `operationId`.
    ///
    /// # Arguments
    ///
    /// * `_params` - Idempotency envelope and tagged operation.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] unless the guest overrides this.
    async fn db_atomic(
        &self,
        _params: DbAtomicRequest,
    ) -> std::result::Result<DbAtomicResult, PluginError> {
        Err(PluginError::unsupported("dbAtomic not implemented"))
    }

    /// Dispatches an unrecognized or future wire method.
    ///
    /// # Arguments
    ///
    /// * `method` - Workers RPC method name from the host.
    /// * `_params` - JSON params object for that method.
    ///
    /// # Returns
    ///
    /// JSON result value for the RPC response.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::unsupported`] for methods the guest does not implement.
    async fn call_raw(
        &self,
        method: &str,
        _params: Value,
    ) -> std::result::Result<Value, PluginError> {
        Err(PluginError::unsupported(format!(
            "method `{method}` not implemented"
        )))
    }
}

/// Maps a plain string into a wire [`PluginError`] with code `internal`.
///
/// Useful when adapting handlers that only produce `Err(String)` into the
/// branded error type expected by hosts.
///
/// # Arguments
///
/// * `message` - Operator-facing explanation (no secrets).
///
/// # Returns
///
/// [`PluginError`] with [`PluginErrorCode::Internal`], empty `details`.
#[must_use]
pub fn plugin_error_from_message(message: String) -> PluginError {
    PluginError::new(PluginErrorCode::Internal, message)
}
