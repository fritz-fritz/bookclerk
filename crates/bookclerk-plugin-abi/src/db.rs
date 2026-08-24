//! Database plugin Workers RPC DTOs (`kind = "database"`).
//!
//! Guests such as `sqlite` / `d1` / `postgres` implement the SeaORM proxy
//! boundary. The host never links SQL engines; it opens the library through
//! these RPC methods after [`crate::methods::db_connect`].
//!
//! | Method | Params | Result |
//! | --- | --- | --- |
//! | [`crate::methods::db_connect`] | [`DbConnectParams`] | [`DbConnectResult`] |
//! | [`crate::methods::db_ping`] | (none) | success / [`crate::PluginError`] |
//! | [`crate::methods::db_query`] | [`StatementDto`] | [`QueryResultDto`] |
//! | [`crate::methods::db_execute`] | [`StatementDto`] | [`ExecResultDto`] |
//! | [`crate::methods::db_begin`] | [`DbBeginParams`] | [`DbBeginResult`] |
//! | [`crate::methods::db_commit`] | [`DbTxnParams`] | success / [`crate::PluginError`] |
//! | [`crate::methods::db_rollback`] | [`DbTxnParams`] | success / [`crate::PluginError`] |
//! | [`crate::methods::db_atomic`] | [`DbAtomicRequest`] | [`DbPlanExecResult`] |
//!
//! Wire fields use camelCase. The `backend` tag on [`DbConnectParams`] is
//! lowercase (`sqlite`, `d1`, `postgres`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// SQL statement plus bind parameters crossing the host↔database-guest boundary.
///
/// Used as params for both [`crate::methods::db_query`] and
/// [`crate::methods::db_execute`]. Bind values are JSON (null, bool, number,
/// string, or nested arrays) matching SeaORM's RPC proxy encoding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StatementDto {
    /// SQL text with positional or named placeholders as understood by the
    /// guest dialect (SQLite `?`, Postgres `$1`, …).
    pub sql: String,
    /// Ordered bind values for the statement (wire `values`; default empty).
    #[serde(default)]
    pub values: Vec<JsonValue>,
    /// Guest transaction id from [`crate::methods::db_begin`] (wire `txnId`).
    ///
    /// Omitted for autocommit statements. When set, the guest runs the
    /// statement inside that transaction (or nested savepoint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txn_id: Option<String>,
}

/// Params for [`crate::methods::db_begin`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbBeginParams {
    /// Existing transaction to nest a savepoint under (wire `parentTxnId`).
    ///
    /// Omitted to start a top-level transaction. The guest serializes
    /// top-level begins so SQLite / D1 never interleave writers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_txn_id: Option<String>,
}

/// Result of a successful [`crate::methods::db_begin`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbBeginResult {
    /// Opaque id the host must send on subsequent statements and
    /// commit/rollback (wire `txnId`).
    pub txn_id: String,
}

/// Params for [`crate::methods::db_commit`] and [`crate::methods::db_rollback`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbTxnParams {
    /// Transaction id returned by [`crate::methods::db_begin`] (wire `txnId`).
    pub txn_id: String,
}

/// One result row from [`crate::methods::db_query`].
///
/// Column names are the keys the guest returns (typically the SQL alias or
/// table column name); values are JSON-encoded cell data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRowDto {
    /// Column name → JSON cell value map for this row (wire `values`).
    pub values: BTreeMap<String, JsonValue>,
}

/// Successful result of [`crate::methods::db_query`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueryResultDto {
    /// Zero or more rows in result-set order.
    pub rows: Vec<ProxyRowDto>,
}

/// Successful result of [`crate::methods::db_execute`] (INSERT/UPDATE/DELETE).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecResultDto {
    /// Last auto-increment / identity value when the backend provides one
    /// (wire `lastInsertId`); `0` when not applicable.
    pub last_insert_id: u64,
    /// Number of rows affected by the statement (wire `rowsAffected`).
    pub rows_affected: u64,
}

/// Tagged connect params for [`crate::methods::db_connect`].
///
/// Discriminant is wire field `backend` with lowercase tags. SQLite guests
/// open `library.db` at [`Self::Sqlite::sqlite_path`] (also injected as
/// `BOOKCLERK_SQLITE_PATH`); D1 / Postgres receive host-injected credentials
/// in the params.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum DbConnectParams {
    /// Local SQLite file backend (`backend: "sqlite"`).
    #[serde(rename_all = "camelCase")]
    Sqlite {
        /// Scoped writable directory for this plugin
        /// (`…/plugins/<id>/data`, wire `pluginDataDir`).
        plugin_data_dir: String,
        /// Absolute path to the DB file (wire `sqlitePath`). The sqlite jail
        /// grants this file and its journal sidecars at spawn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sqlite_path: Option<String>,
    },
    /// Cloudflare D1 HTTP API backend (`backend: "d1"`).
    #[serde(rename_all = "camelCase")]
    D1 {
        /// Scoped writable directory for this plugin (wire `pluginDataDir`).
        plugin_data_dir: String,
        /// Cloudflare account id for the D1 API (wire `accountId`).
        account_id: String,
        /// D1 database UUID (wire `databaseId`).
        database_id: String,
        /// API base URL (for example `https://api.cloudflare.com/client/v4`).
        api_base: String,
        /// Bearer / API token the host injects; guests must not read env for this.
        api_token: String,
    },
    /// PostgreSQL connection-string backend (`backend: "postgres"`).
    #[serde(rename_all = "camelCase")]
    Postgres {
        /// Scoped writable directory for this plugin (wire `pluginDataDir`).
        plugin_data_dir: String,
        /// Full Postgres connection URL (host-injected; may contain secrets).
        url: String,
    },
}

/// Serde default for [`DbConnectResult::interactive_txn`] when older guests omit the field.
fn default_true() -> bool {
    true
}

/// SQLite family bind cap advertised by the platform sqlite guest.
pub const SQLITE_MAX_BINDS: u32 = 32_766;

/// PostgreSQL bind cap advertised by the optional postgres guest.
pub const POSTGRES_MAX_BINDS: u32 = 65_535;

/// Cloudflare D1 bound-parameter limit.
///
/// <https://developers.cloudflare.com/d1/platform/limits/>
pub const D1_MAX_BINDS: u32 = 100;

/// D1 / first-party batch statement cap (D1 HTTP batch is 100 queries).
pub const FIRST_PARTY_MAX_STATEMENTS: u32 = 100;

/// Host refuses guests that cannot bind at least this many parameters.
pub const HOST_MIN_BINDS: u32 = 32;

/// Host refuses guests that cannot run at least this many statements per batch.
pub const HOST_MIN_STATEMENTS: u32 = 40;

/// Host refuses guests that do not bound result rows (`0` is unspecified).
pub const HOST_MIN_RESULT_ROWS: u32 = 1;

/// Host refuses guests that do not bound encoded statement payload bytes.
pub const HOST_MIN_PAYLOAD_BYTES: u32 = 1024;

/// Host refuses guests that do not bound JSON bytes of one statement's rows.
pub const HOST_MIN_RESULT_BYTES: u32 = 4_096;

/// Host refuses guests that do not bound one result cell (`0` is unspecified).
pub const HOST_MIN_CELL_BYTES: u32 = 1_024;

/// First-party JSON-byte budget for one statement's rows and for one atomic
/// request/result scalar. Must stay at or below [`crate::v2::MAX_SCALAR_BYTES`].
pub const FIRST_PARTY_MAX_RESULT_BYTES: u32 = crate::v2::MAX_SCALAR_BYTES;

/// Bookclerk SQL contract version advertised by first-party adapters.
///
/// Contract versions are **monotonic supersets** (see `docs/sql-contract/v1.md`):
/// every guarantee in version *N* remains valid in *N+1*. Guests advertise the
/// highest version they implement; hosts require
/// `sqlContractVersion >= SQL_CONTRACT_VERSION`. A non-superset change must bump
/// this constant and document a new major contract — do not weaken `>=` into a
/// negotiated range until then.
pub const SQL_CONTRACT_VERSION: u32 = 1;

/// Result of a successful [`crate::methods::db_connect`].
///
/// Tells the host which SeaORM dialect to use when composing subsequent
/// `dbQuery` / `dbExecute` statements against this guest, and the negotiated
/// SQL-adapter capabilities. The host must not invent these from the plugin id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbConnectResult {
    /// SeaORM dialect string the host should use for the RPC proxy
    /// (`"sqlite"` or `"postgres"`; D1 guests report `"sqlite"`).
    pub dialect: String,
    /// When `false`, the host must not use SeaORM `begin()` / `dbBegin`.
    ///
    /// SQLite and Postgres default to `true`. D1 HTTP cannot keep `BEGIN`
    /// open across RPCs; those guests set `false` and implement
    /// [`crate::methods::db_atomic`] instead. Omitted on the wire by older
    /// guests (treated as `true`).
    #[serde(default = "default_true")]
    pub interactive_txn: bool,
    /// SQL dialect family for SeaORM proxy bootstrap (`sqlite` or `postgres`).
    ///
    /// Bootstrap-only metadata; host schema and plan selection must not branch
    /// on this field. Empty on older guests.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sql_family: String,
    /// Guest can run a bounded statement list as one SQL transaction.
    #[serde(default = "default_true")]
    pub atomic_batch: bool,
    /// Guest SQL supports `RETURNING` result sets.
    ///
    /// Host plans use `RETURNING` unconditionally. `false` fails capability
    /// negotiation until a real fallback exists.
    #[serde(default = "default_true")]
    pub returning: bool,
    /// Maximum bound parameters per statement (`0` is unspecified and fails closed).
    #[serde(default)]
    pub max_binds: u32,
    /// Maximum statements in one atomic batch (`0` is unspecified and fails closed).
    #[serde(default)]
    pub max_statements: u32,
    /// Maximum rows a query statement may return (`0` is unspecified and fails closed).
    #[serde(default)]
    pub max_result_rows: u32,
    /// Maximum UTF-8 bytes of SQL text plus JSON binds per statement
    /// (`0` is unspecified and fails closed). Must be `<=` [`crate::v2::MAX_SCALAR_BYTES`].
    #[serde(default)]
    pub max_payload_bytes: u32,
    /// Maximum JSON bytes of one statement's result rows
    /// (`0` is unspecified and fails closed).
    #[serde(default)]
    pub max_result_bytes: u32,
    /// Maximum UTF-8 / blob bytes of one result cell
    /// (`0` is unspecified and fails closed).
    #[serde(default)]
    pub max_cell_bytes: u32,
    /// Maximum encoded bytes of one [`ExecuteRequest`]
    /// (`0` is unspecified and fails closed). Must be `<=` [`crate::v2::MAX_SCALAR_BYTES`].
    #[serde(default)]
    pub max_atomic_request_bytes: u32,
    /// Maximum encoded bytes of one atomic execute reply
    /// (`0` is unspecified and fails closed). Must be `<=` [`crate::v2::MAX_SCALAR_BYTES`].
    #[serde(default)]
    pub max_atomic_result_bytes: u32,
    /// Bookclerk SQL contract version (`0` is unspecified and fails closed).
    #[serde(default)]
    pub sql_contract_version: u32,
    /// Guest versions schema with `PRAGMA user_version`.
    #[serde(default)]
    pub pragma_user_version: bool,
    /// Guest versions schema with a `schema_migrations` table.
    #[serde(default)]
    pub schema_migrations: bool,
    /// Each schema version must be applied as one atomic batch (D1 HTTP).
    #[serde(default)]
    pub atomic_schema_batch: bool,
    /// Guest can fill [`DbAtomicTiming::db_execution_us`].
    #[serde(default = "default_true")]
    pub timing: bool,
}

impl DbConnectResult {
    /// Connect result advertising the SQLite dialect with interactive transactions.
    #[must_use]
    pub fn sqlite() -> Self {
        Self {
            dialect: String::from("sqlite"),
            interactive_txn: true,
            sql_family: String::from("sqlite"),
            atomic_batch: true,
            returning: true,
            max_binds: SQLITE_MAX_BINDS,
            max_statements: FIRST_PARTY_MAX_STATEMENTS,
            max_result_rows: 1_000,
            max_payload_bytes: crate::v2::MAX_SCALAR_BYTES,
            max_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_cell_bytes: crate::v2::MAX_SCALAR_BYTES,
            max_atomic_request_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_atomic_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            sql_contract_version: SQL_CONTRACT_VERSION,
            pragma_user_version: true,
            schema_migrations: false,
            atomic_schema_batch: false,
            timing: true,
        }
    }

    /// SQLite-family guest that versions schema with `schema_migrations` rows.
    ///
    /// Host schema *policy* still treats `schemaMigrations=true` plus
    /// `atomicSchemaBatch=false` as the row-based pack. The SeaORM proxy
    /// backend must come from [`Self::sql_family`] (`sqlite`), not from that
    /// versioning kind.
    #[must_use]
    pub fn sqlite_row_migrations() -> Self {
        Self {
            dialect: String::from("sqlite"),
            interactive_txn: true,
            sql_family: String::from("sqlite"),
            atomic_batch: true,
            returning: true,
            max_binds: SQLITE_MAX_BINDS,
            max_statements: FIRST_PARTY_MAX_STATEMENTS,
            max_result_rows: 1_000,
            max_payload_bytes: crate::v2::MAX_SCALAR_BYTES,
            max_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_cell_bytes: crate::v2::MAX_SCALAR_BYTES,
            max_atomic_request_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_atomic_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            sql_contract_version: SQL_CONTRACT_VERSION,
            pragma_user_version: false,
            schema_migrations: true,
            atomic_schema_batch: false,
            timing: true,
        }
    }

    /// Connect result advertising the Postgres dialect with interactive transactions.
    #[must_use]
    pub fn postgres() -> Self {
        Self {
            dialect: String::from("postgres"),
            interactive_txn: true,
            sql_family: String::from("postgres"),
            atomic_batch: true,
            returning: true,
            max_binds: POSTGRES_MAX_BINDS,
            max_statements: FIRST_PARTY_MAX_STATEMENTS,
            max_result_rows: 1_000,
            max_payload_bytes: crate::v2::MAX_SCALAR_BYTES,
            max_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_cell_bytes: crate::v2::MAX_SCALAR_BYTES,
            max_atomic_request_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_atomic_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            sql_contract_version: SQL_CONTRACT_VERSION,
            pragma_user_version: false,
            schema_migrations: true,
            atomic_schema_batch: false,
            timing: true,
        }
    }

    /// Connect result for Cloudflare D1 (SQLite dialect, no interactive `BEGIN`).
    #[must_use]
    pub fn d1() -> Self {
        Self {
            dialect: String::from("sqlite"),
            interactive_txn: false,
            sql_family: String::from("sqlite"),
            atomic_batch: true,
            returning: true,
            max_binds: D1_MAX_BINDS,
            max_statements: FIRST_PARTY_MAX_STATEMENTS,
            max_result_rows: 1_000,
            max_payload_bytes: crate::v2::MAX_SCALAR_BYTES,
            max_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_cell_bytes: crate::v2::MAX_SCALAR_BYTES,
            max_atomic_request_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            max_atomic_result_bytes: FIRST_PARTY_MAX_RESULT_BYTES,
            sql_contract_version: SQL_CONTRACT_VERSION,
            pragma_user_version: false,
            schema_migrations: true,
            atomic_schema_batch: true,
            timing: true,
        }
    }

    /// True when this guest meets the host's compiled minimum SQL contract.
    #[must_use]
    pub fn meets_host_minimums(&self) -> bool {
        self.capability_failure_reason_opt().is_none()
    }

    /// Operator-facing reason when [`Self::meets_host_minimums`] is false.
    #[must_use]
    pub fn capability_failure_reason(&self) -> String {
        self.capability_failure_reason_opt()
            .unwrap_or_else(|| "database guest failed capability negotiation".into())
    }

    /// Failure reason, or `None` when the guest meets host minima.
    fn capability_failure_reason_opt(&self) -> Option<String> {
        if !self.atomic_batch {
            return Some("database guest does not advertise atomicBatch".into());
        }
        if !self.returning {
            return Some(
                "database guest does not advertise returning (host plans require RETURNING)".into(),
            );
        }
        if self.max_binds < HOST_MIN_BINDS {
            return Some(format!(
                "database guest maxBinds {} is below host minimum {HOST_MIN_BINDS}",
                self.max_binds
            ));
        }
        if self.max_statements < HOST_MIN_STATEMENTS {
            return Some(format!(
                "database guest maxStatements {} is below host minimum {HOST_MIN_STATEMENTS}",
                self.max_statements
            ));
        }
        if self.max_result_rows < HOST_MIN_RESULT_ROWS {
            return Some(format!(
                "database guest maxResultRows {} is below host minimum {HOST_MIN_RESULT_ROWS}",
                self.max_result_rows
            ));
        }
        if self.max_payload_bytes < HOST_MIN_PAYLOAD_BYTES
            || self.max_payload_bytes > crate::v2::MAX_SCALAR_BYTES
        {
            return Some(format!(
                "database guest maxPayloadBytes {} must be between {HOST_MIN_PAYLOAD_BYTES} and {}",
                self.max_payload_bytes,
                crate::v2::MAX_SCALAR_BYTES
            ));
        }
        if self.max_result_bytes < HOST_MIN_RESULT_BYTES {
            return Some(format!(
                "database guest maxResultBytes {} is below host minimum {HOST_MIN_RESULT_BYTES}",
                self.max_result_bytes
            ));
        }
        if self.max_cell_bytes < HOST_MIN_CELL_BYTES {
            return Some(format!(
                "database guest maxCellBytes {} is below host minimum {HOST_MIN_CELL_BYTES}",
                self.max_cell_bytes
            ));
        }
        if self.max_atomic_request_bytes < HOST_MIN_RESULT_BYTES
            || self.max_atomic_request_bytes > crate::v2::MAX_SCALAR_BYTES
        {
            return Some(format!(
                "database guest maxAtomicRequestBytes {} must be between {HOST_MIN_RESULT_BYTES} and {}",
                self.max_atomic_request_bytes,
                crate::v2::MAX_SCALAR_BYTES
            ));
        }
        if self.max_atomic_result_bytes < HOST_MIN_RESULT_BYTES
            || self.max_atomic_result_bytes > crate::v2::MAX_SCALAR_BYTES
        {
            return Some(format!(
                "database guest maxAtomicResultBytes {} must be between {HOST_MIN_RESULT_BYTES} and {}",
                self.max_atomic_result_bytes,
                crate::v2::MAX_SCALAR_BYTES
            ));
        }
        if self.max_result_bytes > self.max_atomic_result_bytes {
            return Some(format!(
                "database guest maxResultBytes {} exceeds maxAtomicResultBytes {}",
                self.max_result_bytes, self.max_atomic_result_bytes
            ));
        }
        if self.sql_contract_version < SQL_CONTRACT_VERSION {
            return Some(format!(
                "database guest sqlContractVersion {} is below host minimum {SQL_CONTRACT_VERSION}",
                self.sql_contract_version
            ));
        }
        if self.pragma_user_version == self.schema_migrations {
            return Some(
                "database guest must advertise exactly one of pragmaUserVersion or schemaMigrations"
                    .into(),
            );
        }
        if self.atomic_schema_batch && !self.schema_migrations {
            return Some("database guest atomicSchemaBatch requires schemaMigrations".into());
        }
        None
    }

    /// SeaORM proxy backend failure from bootstrap metadata (`dialect` / `sqlFamily`).
    #[must_use]
    pub fn bootstrap_backend_failure_reason(&self) -> Option<String> {
        let family = self.sql_family.to_ascii_lowercase();
        if !family.is_empty() {
            if family != "sqlite" && family != "postgres" {
                return Some(format!(
                    "database guest sqlFamily {:?} is not sqlite or postgres (SQL-like backends only)",
                    self.sql_family
                ));
            }
            if !self.dialect.is_empty() && !dialect_matches_sql_family(&self.dialect, &family) {
                return Some(format!(
                    "database guest dialect {:?} does not match sqlFamily {:?}",
                    self.dialect, self.sql_family
                ));
            }
            return None;
        }
        let dialect = self.dialect.to_ascii_lowercase();
        if dialect.is_empty() {
            return Some("database guest dialect is required for SeaORM proxy bootstrap".into());
        }
        if dialect == "sqlite"
            || dialect == "postgres"
            || dialect == "postgresql"
            || dialect == "pg"
        {
            return None;
        }
        Some(format!(
            "database guest dialect {:?} is not sqlite or postgres (SQL-like backends only)",
            self.dialect
        ))
    }
}

/// True when SeaORM `dialect` names the same SQL family as `sql_family`.
fn dialect_matches_sql_family(dialect: &str, sql_family: &str) -> bool {
    match sql_family {
        "sqlite" => dialect.eq_ignore_ascii_case("sqlite"),
        "postgres" => {
            dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql")
        }
        _ => false,
    }
}
