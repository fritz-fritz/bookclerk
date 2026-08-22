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
//! | [`crate::methods::db_atomic`] | [`DbAtomicRequest`] | [`DbAtomicResult`] |
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

/// Sentinel SQL for [`crate::methods::db_atomic`] (`DatabaseSession.query`).
pub const DB_ATOMIC_SENTINEL: &str = "bookclerk.atomic";

/// Sentinel SQL for capability negotiation (`DatabaseSession.query` after open).
pub const DB_CAPABILITIES_SENTINEL: &str = "bookclerk.capabilities";

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
    /// SQL dialect family for host-authored plans (`sqlite` or `postgres`).
    ///
    /// Empty on older guests; the host then fails closed for generic plans.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sql_family: String,
    /// Guest can run a bounded statement list as one SQL transaction.
    #[serde(default = "default_true")]
    pub atomic_batch: bool,
    /// Guest SQL supports `RETURNING` result sets.
    #[serde(default = "default_true")]
    pub returning: bool,
    /// Maximum bound parameters per statement (`0` means unspecified).
    #[serde(default)]
    pub max_binds: u32,
    /// Maximum statements in one atomic batch (`0` means unspecified).
    #[serde(default)]
    pub max_statements: u32,
    /// Maximum rows a query statement may return (`0` means unspecified).
    #[serde(default)]
    pub max_result_rows: u32,
    /// Maximum UTF-8 bytes of SQL text plus JSON binds per statement.
    #[serde(default)]
    pub max_payload_bytes: u32,
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
            max_payload_bytes: 1_048_576,
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
            max_payload_bytes: 1_048_576,
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
            max_payload_bytes: 1_048_576,
            timing: true,
        }
    }

    /// True when this guest meets the host's compiled minimum SQL contract.
    #[must_use]
    pub fn meets_host_minimums(&self) -> bool {
        self.atomic_batch
            && (self.sql_family == "sqlite" || self.sql_family == "postgres")
            && self.max_binds >= HOST_MIN_BINDS
            && self.max_statements >= HOST_MIN_STATEMENTS
    }

    /// Operator-facing reason when [`Self::meets_host_minimums`] is false.
    #[must_use]
    pub fn capability_failure_reason(&self) -> String {
        if !self.atomic_batch {
            return "database guest does not advertise atomicBatch".into();
        }
        if self.sql_family != "sqlite" && self.sql_family != "postgres" {
            return format!(
                "database guest sqlFamily {:?} is not sqlite or postgres (SQL-like backends only)",
                self.sql_family
            );
        }
        if self.max_binds < HOST_MIN_BINDS {
            return format!(
                "database guest maxBinds {} is below host minimum {HOST_MIN_BINDS}",
                self.max_binds
            );
        }
        if self.max_statements < HOST_MIN_STATEMENTS {
            return format!(
                "database guest maxStatements {} is below host minimum {HOST_MIN_STATEMENTS}",
                self.max_statements
            );
        }
        "database guest failed capability negotiation".into()
    }
}

/// Application status strings returned by [`crate::methods::db_atomic`].
pub mod atomic_status {
    /// Operation committed with the expected payload (or no payload).
    pub const OK: &str = "ok";
    /// Consume-once lookup found nothing (or the row was expired).
    pub const EMPTY: &str = "empty";
    /// Target row does not exist.
    pub const NOT_FOUND: &str = "notFound";
    /// Refused: would remove the last active owner.
    pub const LAST_OWNER: &str = "lastOwner";
    /// Claim ticket missing, expired, or already redeemed.
    pub const CLAIM_INVALID: &str = "claimInvalid";
    /// Local claim login needs a password; the ticket was not consumed.
    pub const PASSWORD_REQUIRED: &str = "passwordRequired";
    /// Same `operationId` reused with a different request body.
    pub const IDEMPOTENCY_CONFLICT: &str = "idempotencyConflict";
    /// Job admit found an equivalent pending/running row.
    pub const DUPLICATE: &str = "duplicate";
    /// Job admit refused because the pending+running cap was reached.
    pub const QUEUE_FULL: &str = "queueFull";
}

/// How a guest should run one statement inside an atomic plan.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DbPlanStatementKind {
    /// Statement returns rows (SELECT / RETURNING).
    #[default]
    Query,
    /// Statement is DML; only `rowsAffected` is required.
    Execute,
}

/// One parameterized statement in a host-authored [`DbAtomicPlan`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbPlanStatement {
    /// Dialect-specific SQL (SQLite `?` or Postgres `$1`).
    pub sql: String,
    /// Ordered JSON binds (null, bool, number, string, `b64:` blobs, or
    /// [`sea_null`] objects so Postgres can distinguish BYTEA/INTEGER nulls).
    #[serde(default)]
    pub binds: Vec<JsonValue>,
    /// Whether the guest should collect rows or only `rowsAffected`.
    #[serde(default)]
    pub kind: DbPlanStatementKind,
}

/// JSON object key for a typed SQL null (`{"$sea_null": "Bytes"}`).
pub const SEA_NULL_KEY: &str = "$sea_null";

/// Wire JSON for a typed null bind of SeaORM `kind` (`Bytes`, `BigInt`, `String`, …).
///
/// Postgres infers parameter types from the bind OID. A JSON `null` that
/// becomes `Value::String(None)` cannot be inserted into `BYTEA` or `INTEGER`.
#[must_use]
pub fn sea_null(kind: &str) -> JsonValue {
    let mut map = serde_json::Map::new();
    map.insert(SEA_NULL_KEY.into(), JsonValue::String(kind.to_string()));
    JsonValue::Object(map)
}

/// Returns the `$sea_null` kind when `v` is a typed-null object.
#[must_use]
pub fn sea_null_kind(v: &JsonValue) -> Option<&str> {
    v.get(SEA_NULL_KEY)?.as_str()
}

/// Generic atomic batch: ordered SQL with outcome/receipt selectors.
///
/// The plan describes database work, not Bookclerk operations. Domain names
/// stay in host code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbAtomicPlan {
    /// Statements run as one SQL transaction, in order.
    pub statements: Vec<DbPlanStatement>,
    /// Index of the application-status `SELECT` when receipts are not used.
    #[serde(default)]
    pub outcome_index: u32,
    /// Index of a payload `SELECT` when the op returns a JSON record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_index: Option<u32>,
    /// Index of the receipt `SELECT` immediately after prune (replay detect).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_receipt_index: Option<u32>,
    /// Index of the final receipt `SELECT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_select_index: Option<u32>,
}

/// Named atomic library operation compiled by the host into a generic plan.
///
/// Domain names stay in host code. Database guests execute
/// [`DbAtomicPlan`] statements and must not match on this enum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum DbAtomicParams {
    /// Delete a first-party user and personal data (last-owner guarded).
    #[serde(rename_all = "camelCase")]
    DeleteUser {
        /// `users.id` to delete.
        user_id: i64,
    },
    /// Set `users.status` (`active` / `disabled`; last-owner guarded).
    #[serde(rename_all = "camelCase")]
    SetUserStatus {
        /// `users.id` to update.
        user_id: i64,
        /// Canonical status string (`active` or `disabled`).
        status: String,
    },
    /// Set or clear the Argon2id password hash and bump `security_version`.
    #[serde(rename_all = "camelCase")]
    SetUserPasswordHash {
        /// `users.id` to update.
        user_id: i64,
        /// New hash, or `null` to clear.
        password_hash: Option<String>,
    },
    /// Set `users.role` (last-owner guarded on demotion).
    #[serde(rename_all = "camelCase")]
    SetUserRole {
        /// `users.id` to update.
        user_id: i64,
        /// Canonical role string (`owner` / `administrator` / `member`).
        role: String,
    },
    /// Consume a claim ticket, optionally set a first password, mint a session.
    #[serde(rename_all = "camelCase")]
    RedeemClaimTicket {
        /// SHA-256 hex digest of the claim ticket.
        token_hash: String,
        /// SHA-256 hex digest of the new portal session token.
        session_hash: String,
        /// RFC 3339 expiry for the minted session.
        expires_at: String,
        /// Raw User-Agent captured at session mint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_agent: Option<String>,
        /// Best-effort device class.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_type: Option<String>,
        /// Best-effort OS / client label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_label: Option<String>,
        /// Argon2id hash to set when the local user has none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_password_hash: Option<String>,
        /// Domain-separated HMAC of the invite password for idempotency.
        ///
        /// Argon2id salts change on every POST; this fingerprint is stable across
        /// retries and is not stored as the password hash.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password_fingerprint: Option<String>,
    },
    /// Consume a one-time OIDC RP state (`DELETE` + expiry check).
    #[serde(rename_all = "camelCase")]
    TakeOidcRpState {
        /// SHA-256 hex digest of the OAuth `state` parameter.
        state_hash: String,
    },
    /// Consume a one-time WebAuthn challenge (`DELETE` + expiry check).
    #[serde(rename_all = "camelCase")]
    TakeWebauthnChallenge {
        /// Public ceremony id returned to the browser.
        challenge_id: String,
        /// `register`, `login`, or `elevate`.
        kind: String,
    },
    /// Admit a durable job (dedup + pending cap + insert).
    #[serde(rename_all = "camelCase")]
    EnqueueJob {
        /// Job kind wire string (`scan`, `acquire`, `listen_sync`, `integration_scan`).
        kind: String,
        /// Versioned command envelope JSON.
        payload_json: String,
        /// Higher values are claimed first.
        priority: i64,
        /// Maximum claims before a failure is terminal.
        max_attempts: i64,
        /// Global cap on pending+running rows.
        max_pending: i64,
        /// Optional RFC 3339 delay before the job becomes claimable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_after: Option<String>,
    },
    /// Claim the next ready job in `resource_class` for `owner`.
    #[serde(rename_all = "camelCase")]
    ClaimNextJob {
        /// Resource class wire string (`network`, …).
        resource_class: String,
        /// Worker id stored as `lease_owner`.
        owner: String,
        /// Lease length in seconds.
        lease_secs: i64,
    },
    /// Reserve scratch-quota bytes for one job path.
    #[serde(rename_all = "camelCase")]
    ReserveJobTemp {
        /// Owning job id.
        job_id: String,
        /// Absolute filesystem path.
        path: String,
        /// Bytes to reserve for this path.
        reserved_bytes: i64,
        /// Global reserved-bytes cap.
        quota_bytes: i64,
    },
    /// Promote a sealed TOTP secret to `primary` and set `users.totp_enabled`.
    ///
    /// The host seals with the process DEK first. Ciphertext, nonce, and salt
    /// are `b64:`-prefixed strings (same encoding as D1 BLOB binds).
    #[serde(rename_all = "camelCase")]
    ConfirmTotpEnrollment {
        /// `users.id` to enroll.
        user_id: i64,
        /// Payload format (`sealed-v1`).
        format: String,
        /// Sealed TOTP secret bytes (`b64:…`).
        ciphertext: String,
        /// Cipher algorithm identifier, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cipher_algorithm: Option<String>,
        /// AEAD nonce (`b64:…`), if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cipher_nonce: Option<String>,
        /// Legacy KDF algorithm, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kdf_algorithm: Option<String>,
        /// Legacy KDF salt (`b64:…`), if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kdf_salt: Option<String>,
        /// Legacy Argon2 memory cost in KiB, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kdf_m_cost: Option<i64>,
        /// Legacy Argon2 time cost, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kdf_t_cost: Option<i64>,
        /// Legacy Argon2 parallelism, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kdf_p_cost: Option<i64>,
        /// RFC 3339 timestamp to store as `created_at` on the primary row.
        created_at: String,
    },
    /// Delete TOTP secrets and clear `users.totp_enabled`.
    #[serde(rename_all = "camelCase")]
    DisableUserTotp {
        /// `users.id` to disable.
        user_id: i64,
    },
    /// Persist a domain event in the outbox (dedup on eventType+dedupKey).
    #[serde(rename_all = "camelCase")]
    PublishDomainEvent {
        /// Stable event id (UUID).
        id: String,
        /// Event type (`book_acquired`).
        event_type: String,
        /// Payload schema version.
        schema_version: i64,
        /// Tenant / account id.
        #[serde(default)]
        account_id: String,
        /// Producer plugin id; empty when unknown.
        #[serde(default)]
        source: String,
        /// Trace correlation id.
        #[serde(default)]
        correlation_id: String,
        /// Causing event or job id.
        #[serde(default)]
        causation_id: String,
        /// Unique with `eventType`.
        dedup_key: String,
        /// Bounded JSON payload.
        payload: String,
        /// FIFO key copied onto deliveries.
        #[serde(default)]
        ordering_key: String,
    },
    /// Update a book row and optionally publish `book_acquired` in the same batch.
    #[serde(rename_all = "camelCase")]
    SetAcquireStatus {
        /// Book UUID to update.
        book_uuid: String,
        /// Acquire status string (`acquired`, `downloading`, …).
        status: String,
        /// Object-storage key for the primary audio artifact.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        storage_key: Option<String>,
        /// Optional failure message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_message: Option<String>,
        /// Event id when publishing; empty skips the outbox insert.
        #[serde(default)]
        event_id: String,
        /// Event type when publishing (`book_acquired`).
        #[serde(default)]
        event_type: String,
        /// Payload schema version.
        #[serde(default)]
        schema_version: i64,
        /// Tenant / account id on the event.
        #[serde(default)]
        event_account_id: String,
        /// Producer plugin id; empty when unknown.
        #[serde(default)]
        source: String,
        /// Trace correlation id.
        #[serde(default)]
        correlation_id: String,
        /// Causing event or job id.
        #[serde(default)]
        causation_id: String,
        /// Unique with `eventType`.
        #[serde(default)]
        dedup_key: String,
        /// Bounded JSON payload.
        #[serde(default)]
        payload: String,
        /// FIFO key copied onto deliveries.
        #[serde(default)]
        ordering_key: String,
    },
    /// Create per-subscriber deliveries and mark the event dispatched.
    #[serde(rename_all = "camelCase")]
    DispatchEventDeliveries {
        /// Parent event id.
        event_id: String,
        /// JSON array of `{ "pluginId": "…" }` subscriber snapshots.
        subscribers_json: String,
        /// When true, this page is the last snapshot and may mark the parent dispatched.
        #[serde(default = "default_mark_dispatched")]
        mark_dispatched: bool,
    },
    /// Claim the next ready event delivery for `owner`.
    #[serde(rename_all = "camelCase")]
    ClaimNextEventDelivery {
        /// Worker id stored as `lease_owner`.
        owner: String,
        /// Lease length in seconds.
        lease_secs: i64,
        /// JSON array of plugin ids this worker can execute (`[]` claims nothing).
        #[serde(default)]
        plugin_ids_json: String,
        /// Cluster-wide max `running` deliveries per `(plugin_id, resource_class)`.
        #[serde(default = "default_claim_max_in_flight")]
        max_in_flight: i64,
    },
}

/// Default cluster in-flight cap when an older guest omits the field.
fn default_claim_max_in_flight() -> i64 {
    1
}

/// Older callers omit `markDispatched`; a single-page dispatch still finishes the parent.
fn default_mark_dispatched() -> bool {
    true
}

/// Host-generated idempotency envelope for [`crate::methods::db_atomic`].
///
/// Guests execute [`Self::plan`] as one SQL transaction and must not parse
/// Bookclerk operation names. [`Self::operation`] is a host compiler input and
/// is omitted from the wire JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbAtomicRequest {
    /// Caller-chosen idempotency key (UUID). Retries must reuse the same id.
    pub operation_id: String,
    /// Named library operation used by the host planner (not sent to guests).
    #[serde(default, skip_serializing)]
    pub operation: Option<DbAtomicParams>,
    /// SHA-256 hex of the idempotency-relevant request; compared on receipt replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    /// Generic statement plan. Guests fail closed when this is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<DbAtomicPlan>,
}

impl DbAtomicRequest {
    /// Envelope for a named library operation (compatibility window).
    #[must_use]
    pub fn named(operation_id: impl Into<String>, operation: DbAtomicParams) -> Self {
        Self {
            operation_id: operation_id.into(),
            operation: Some(operation),
            request_hash: None,
            plan: None,
        }
    }

    /// Envelope for a host-authored generic plan.
    #[must_use]
    pub fn with_plan(
        operation_id: impl Into<String>,
        request_hash: impl Into<String>,
        plan: DbAtomicPlan,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            operation: None,
            request_hash: Some(request_hash.into()),
            plan: Some(plan),
        }
    }
}

/// Optional engine timing for [`DbAtomicResult`]. Not part of the idempotency hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DbAtomicTiming {
    /// Monotonic duration of this plugin-handler attempt.
    pub attempt_elapsed_us: u64,
    /// Engine-reported SQL/transaction time when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_execution_us: Option<u64>,
    /// How `db_execution_us` was measured (`d1_sql_duration`, `sqlite_txn`, `postgres_txn`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_timing_source: Option<String>,
}

/// Result of a successful [`crate::methods::db_atomic`] RPC.
///
/// `status` is an application outcome ([`atomic_status`]); SQL failures are
/// plugin errors and roll back the D1 batch. `payload` is a library record
/// JSON object using snake_case field names matching `UserRecord` /
/// `PortalIdentity`. Receipt metadata lets the host replay a committed
/// operation after an ambiguous transport error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbAtomicResult {
    /// Application outcome (`ok`, `empty`, `notFound`, `lastOwner`, …).
    pub status: String,
    /// Op-specific record when `status` is `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<JsonValue>,
    /// Echo of the request `operationId`.
    #[serde(default)]
    pub operation_id: String,
    /// True when this result was loaded from a durable receipt.
    #[serde(default)]
    pub replayed: bool,
    /// RFC 3339 timestamp when the receipt was first written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_created_at: Option<String>,
    /// Handler/engine timing. Not hashed for idempotency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<DbAtomicTiming>,
}

impl DbAtomicResult {
    /// Successful operation with a JSON payload.
    #[must_use]
    pub fn ok(payload: JsonValue) -> Self {
        Self {
            status: atomic_status::OK.into(),
            payload: Some(payload),
            operation_id: String::new(),
            replayed: false,
            receipt_created_at: None,
            timing: None,
        }
    }

    /// Successful operation with no payload (`deleteUser`).
    #[must_use]
    pub fn ok_unit() -> Self {
        Self {
            status: atomic_status::OK.into(),
            payload: None,
            operation_id: String::new(),
            replayed: false,
            receipt_created_at: None,
            timing: None,
        }
    }

    /// Application failure or empty consume-once result (no payload).
    #[must_use]
    pub fn with_status(status: &str) -> Self {
        Self {
            status: status.to_string(),
            payload: None,
            operation_id: String::new(),
            replayed: false,
            receipt_created_at: None,
            timing: None,
        }
    }
}
