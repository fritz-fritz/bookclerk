//! Guest-only Bookclerk plugin SDK.
//!
//! # Audience
//!
//! Third-party **Rust plugin authors**. Depend on this crate (path/git today;
//! crates.io later) — not the host `bookclerk-plugin-host` crate, which pulls
//! library/config and other Bookclerk internals.
//!
//! # What to use
//!
//! | Need | Entry point |
//! | --- | --- |
//! | Native guest (`runtime = "native"`) | [`PluginRoot`] / [`serve`] (`api_version = 2`) |
//! | Fetch / upload work paths | [`fetch_work_dir`], [`upload_file_path`] |
//! | OAuth callback without guest listen | [`callback_tunnel`] |
//! | Workerd / Wasm guests | [`workerd`] + npm `@bookclerk/plugin-sdk` |
//! | ABI DTOs / method names | [`protocol`] (re-exports `bookclerk-plugin-abi`) |
//! | Database guest session / atomic execution | feature `db` → [`database_adapter`] |
//! | Author CLI (`check` / `fmt` / `package` / `smoke`) | feature `tools` → [`tools`] |
//!
//! ```toml
//! # In a standalone plugin repo / workspace:
//! [dependencies]
//! bookclerk-plugin-sdk = { git = "https://github.com/fritz-fritz/bookclerk", package = "bookclerk-plugin-sdk" }
//! # Database guests also need:
//! # bookclerk-plugin-sdk = { …, features = ["db"] }
//! ```
//!
//! Authoring helpers (`check` / `fmt` / `package` / `smoke`) live behind feature
//! `tools` (pulls `bookclerk-workerd` for smoke only). Guest plugins should use
//! default features.
//!
//! ```bash
//! cargo run -p bookclerk-plugin-sdk --features tools --bin bookclerk-plugin -- check .
//! cargo plugin -- smoke .   # alias enables --features tools
//! ```
//!
//! # API documentation
//!
//! Generated rustdoc for this crate is part of the workspace API reference
//! (`./scripts/generate-api-docs.sh` → `docs/api/rust/`). Style guide:
//! `docs/code-documentation.md`. Product narrative: `docs/plugins.md`,
//! `docs/plugin-registry.md`.
//!
//! # Re-exports
//!
//! Crate-root `pub use` items are the stable import surface for guests
//! (handshake DTOs, [`PluginError`], fetch path helpers, tunnel types). Prefer
//! `use bookclerk_plugin_sdk::…` over reaching into submodules unless you need
//! module-level docs.

pub mod callback_tunnel;
#[cfg(feature = "db")]
pub mod database_adapter;
#[cfg(feature = "db")]
mod db;
mod db_binding;
mod error;
mod fetch_dir;
mod pass_fd;
pub mod protocol;
pub mod tools;
pub mod v2;
pub mod workerd;

pub use callback_tunnel::{TunnelGuest, TunnelHost, TunnelStream};
#[cfg(feature = "db")]
pub use db::{db_value_from_sea, proxy_rows_from_typed};
pub use db_binding::{
    execute_reply_to_d1_results, statement_result_to_d1_result, D1ExecResult, D1Meta, D1Result,
    DatabaseBinding, DatabaseBindingOptions, PreparedStatement, RetryToken,
};
pub use error::{Result, SdkError};
pub use fetch_dir::{fetch_work_dir, upload_file_path, FetchWorkDir, UploadFile};
pub use pass_fd::{fd_proc_path, recv_passed_fd, PLUGIN_FD_CHANNEL, PLUGIN_FD_CHANNEL_ENV};
pub use protocol::{
    methods, AuthenticateUserParams, BookAcquiredDto, BrandDto, CatalogDetailParams, CatalogHitDto,
    CliArgKind, CliArgSpec, CliCommandSpec, CliInvokeParams, CliInvokeResult, CliSchema,
    ConfigOptionDto, ConfigOptionValueDto, CopyParams, CredentialsUpdateParams, EventPollResultDto,
    ExistsResultDto, ExpandCandidatesParams, ExternalUserDto, FetchTitleParams, GetParams,
    GetResultDto, HandshakeResult, HealthDto, HealthResult, KeyParams, ListAccountsParams,
    ListDealsParams, ListParams, ListeningProgressDto, LocalCopyParams, LocalGetParams,
    LocalKeyParams, LocalListParams, LocalPutFileParams, LocalPutParams, LocalTouchFileParams,
    LoginCompleteParams, LoginParams, LoginResultDto, LoginStartParams, LoginStartResultDto,
    ObjectInfoDto, ObjectMetaDto, ObjectProbeDto, OutputCopyParams, OutputGetParams,
    OutputKeyParams, OutputListParams, OutputLocalContextDto, OutputPutFileParams, OutputPutParams,
    OutputS3ContextDto, OutputTouchFileParams, PlainPartDto, PurchaseHintDto, PurchaseHintParams,
    PutFileParams, PutParams, S3CredentialsDto, ScanBookDto, ScanLibraryParams, ScanParams,
    ScanSummaryDto, SearchCatalogParams, SourceAccountDto, SourceFetchDto, SyncListeningResultDto,
    TouchFileParams, HOST_MANIFEST_API_VERSION_MAX, MAX_RPC_LINE_BYTES, PLUGIN_API_VERSION,
    PROTOCOL_NAME,
};
pub use v2::{
    decode_json, encode_atomic_result, encode_json, page_rows, serve, serve_v2,
    AdapterDatabaseSession, ContentSource, Database, GuestDatabase, Integration, PluginDescribe,
    PluginRoot, FEATURE_SCALAR_LIMITS, PRODUCT_API_VERSION,
};

pub use bookclerk_plugin_abi::{
    canonical_execute_request_hash, decode_db_value_bytes, decode_execute_request_bytes,
    decode_execute_result_reply_bytes, encoded_db_value_bytes, encoded_execute_reply_bytes,
    encoded_execute_request_bytes, encoded_execute_result_reply_bytes,
    encoded_statement_result_bytes, sql_payload_bytes, sql_payload_exceeds, DbBootstrap,
    DbCapabilities, DbColumn, DbPlanStatementKind, DbResultSelection, DbRow, DbTiming, DbType,
    DbValue, DiagnoseResult, ExecuteReply, ExecuteRequest, HandshakeParams, HostToPluginEvent,
    PluginError, PluginErrorCode, PluginToHostEvent, StatementResult, TypedDbStatement,
    API_VERSION, SQL_CONTRACT_VERSION,
};
