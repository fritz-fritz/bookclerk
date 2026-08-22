//! Authoritative Bookclerk plugin ABI (`api_version` 2).
//!
//! Version 2 is the product object-capability ABI (Cap'n Proto RPC, role
//! classes, transferred byte streams). JSON DTOs in this crate remain as a
//! versioned escape hatch for plugin-specific config and wrapped guest
//! internals — not as a spawn handshake.
//!
//! # Audience
//!
//! - **Guest authors** — implement Workers RPC methods against these DTOs
//!   (via `bookclerk-plugin-sdk`, `@bookclerk/plugin-sdk`, or a language binding
//!   generated from the same schema).
//! - **Host / SDK maintainers** — deserialize stdio or workerd frames, seal
//!   credentials, and upsert library rows without depending on store-specific
//!   crates.
//!
//! Product narrative (jail, consent, install layout) lives in
//! [`docs/plugins.md`](https://github.com/bookclerk/bookclerk/blob/main/docs/plugins.md).
//! This crate is the typed wire contract only.
//!
//! # Schema
//!
//! The JSON Schema at [`schema/abi.json`](https://github.com/bookclerk/bookclerk/blob/main/crates/bookclerk-plugin-abi/schema/abi.json)
//! (also embedded as [`ABI_SCHEMA_JSON`]) is the canonical contract. Types here
//! are the Rust projection used by host and guest SDKs. Wire DTO fields
//! serialize as **camelCase** to match Workers RPC / TypeScript (`abi.json`
//! `$defs`). Method names on the wire are camelCase strings listed in
//! [`methods::METHOD_NAMES`] (for example `loginStart`, `fetchTitle`,
//! `dbConnect`).
//!
//! Install manifests validate against [`PLUGIN_TOML_SCHEMA_JSON`]
//! (`schema/plugin-toml.json`).
//!
//! # Versioning
//!
//! [`API_VERSION`] is the JSON DTO schema version used by wrapped guest
//! handshake payloads. [`v2::PRODUCT_API_VERSION`] is `2` (object-capability
//! Cap'n Proto / Workers RPC). Product spawn requires `plugin.toml`
//! `api_version = 2`. There is no `protocol` key.
//!
//! # Modules
//!
//! | Module | Contents |
//! | --- | --- |
//! | [`methods`] | Wire method name constants (`handshake`, `onEvent`, …) |
//! | [`types`] | Shared DTOs (handshake, health, CLI, stdio RPC frames) |
//! | [`kind`] | Kind-specific DTOs (source / integration / output) |
//! | [`db`] | Database-guest connect / query / execute DTOs |
//! | [`error`] | [`PluginError`] / [`PluginErrorCode`] |
//! | [`v2`] | Object-capability ABI (`apiVersion` 2, Cap'n Proto, streams) |

pub mod db;
pub mod db_execute;
pub mod db_value;
pub mod error;
pub mod events;
pub mod kind;
pub mod methods;
pub mod types;
pub mod v2;

/// Generated Cap'n Proto RPC interfaces (`schema/plugin_v2.capnp`).
///
/// Included at crate root because `capnpc` emits `crate::plugin_v2_capnp` paths.
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    unused_parens,
    clippy::all,
    clippy::pedantic,
    rustdoc::all,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::missing_docs_in_private_items
)]
pub mod plugin_v2_capnp {
    include!(concat!(env!("OUT_DIR"), "/plugin_v2_capnp.rs"));
}

#[cfg(test)]
mod wire_fixtures;

pub use db::{
    sea_null, sea_null_kind, DbAtomicPlan, DbAtomicRequest, DbAtomicTiming, DbBeginParams,
    DbBeginResult, DbConnectParams, DbConnectResult, DbPlanExecResult, DbPlanStatement,
    DbPlanStatementKind, DbPlanStmtExecResult, DbTxnParams, ExecResultDto, ProxyRowDto,
    QueryResultDto, StatementDto, D1_MAX_BINDS, DB_ATOMIC_SENTINEL, DB_CAPABILITIES_SENTINEL,
    FIRST_PARTY_MAX_RESULT_BYTES, FIRST_PARTY_MAX_STATEMENTS, HOST_MIN_BINDS, HOST_MIN_CELL_BYTES,
    HOST_MIN_PAYLOAD_BYTES, HOST_MIN_RESULT_BYTES, HOST_MIN_RESULT_ROWS, HOST_MIN_STATEMENTS,
    POSTGRES_MAX_BINDS, SEA_NULL_KEY, SQLITE_MAX_BINDS, SQL_CONTRACT_VERSION,
};
pub use db_execute::{
    sql_payload_bytes, sql_payload_exceeds, DbCapabilities, DbColumn, DbResultSelection, DbRow,
    DbTiming, ExecuteReply, ExecuteRequest, StatementResult, TypedDbStatement,
};
pub use db_value::{db_value_from_json, db_value_to_json, DbType, DbValue};
pub use error::{PluginError, PluginErrorCode, Result};
pub use events::{HostToPluginEvent, PluginToHostEvent};
pub use kind::*;
pub use methods::METHOD_NAMES;
pub use types::*;

/// Negotiated JSON-adapter API version (`1`).
///
/// Sent as wire field `apiVersion` on v1 handshake params/results. Product
/// object-capability guests use [`v2::PRODUCT_API_VERSION`] (`2`) instead.
pub const API_VERSION: u32 = 1;

/// Embedded bytes of `schema/abi.json` (CI and docs tooling can compare
/// generators against this exact string).
pub const ABI_SCHEMA_JSON: &str = include_str!("../schema/abi.json");

/// Embedded JSON Schema for install `plugin.toml` files (shared with language
/// SDK author tools and `bookclerk plugins` validation).
pub const PLUGIN_TOML_SCHEMA_JSON: &str = include_str!("../schema/plugin-toml.json");

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;

    #[test]
    fn schema_parses_as_json() {
        let v: serde_json::Value =
            serde_json::from_str(ABI_SCHEMA_JSON).expect("abi.json must be valid JSON");
        assert_eq!(v["title"], "BookclerkPluginAbi");
    }

    #[test]
    fn sea_null_wire_shape() {
        let v = sea_null("Bytes");
        assert_eq!(v, serde_json::json!({ "$sea_null": "Bytes" }));
        assert_eq!(sea_null_kind(&v), Some("Bytes"));
        assert_eq!(sea_null_kind(&serde_json::json!(null)), None);
    }

    #[test]
    fn handshake_roundtrip_camel_case() {
        let hs = HandshakeResult {
            api_version: API_VERSION,
            id: "echo".into(),
            kind: "integration".into(),
            capabilities: vec!["health".into()],
            ..HandshakeResult::default()
        };
        let v = serde_json::to_value(&hs).unwrap();
        assert!(v.get("apiVersion").is_some());
        assert!(v.get("api_version").is_none());
        let back: HandshakeResult = serde_json::from_value(v).unwrap();
        assert_eq!(back.id, "echo");
    }

    #[test]
    fn method_names_match_schema() {
        let v: serde_json::Value = serde_json::from_str(ABI_SCHEMA_JSON).unwrap();
        let mut schema_names: Vec<&str> = v["properties"]["methods"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        let mut expected: Vec<&str> = METHOD_NAMES.to_vec();
        schema_names.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            schema_names, expected,
            "abi.json methods keys must match methods.rs METHOD_NAMES"
        );
    }

    #[test]
    fn db_atomic_plan_wire_omits_named_operations() {
        let plan_req = DbAtomicRequest::with_plan(
            "op-plan",
            "abc",
            DbAtomicPlan {
                statements: vec![DbPlanStatement {
                    sql: "SELECT 1".into(),
                    binds: vec![],
                    kind: DbPlanStatementKind::Query,
                    max_rows: 0,
                }],
                outcome_index: 0,
                payload_index: None,
                prior_receipt_index: None,
                receipt_select_index: None,
            },
        );
        let pv = serde_json::to_value(&plan_req).unwrap();
        assert_eq!(pv["operationId"], "op-plan");
        assert_eq!(pv["requestHash"], "abc");
        assert_eq!(pv["plan"]["statements"][0]["sql"], "SELECT 1");
        assert_eq!(pv["plan"]["statements"][0]["kind"], "query");
        assert!(
            pv.get("operation").is_none(),
            "named operations stay off the guest wire"
        );

        let exec = DbPlanExecResult {
            operation_id: "op-plan".into(),
            statements: vec![DbPlanStmtExecResult {
                rows: vec![serde_json::json!({"status": "ok"})],
                rows_affected: 0,
            }],
            timing: None,
        };
        let ev = serde_json::to_value(&exec).unwrap();
        assert_eq!(ev["operationId"], "op-plan");
        assert_eq!(ev["statements"][0]["rowsAffected"], 0);
        assert!(ev.get("status").is_none());

        let d1 = DbConnectResult::d1();
        let rv = serde_json::to_value(&d1).unwrap();
        assert_eq!(rv["dialect"], "sqlite");
        assert_eq!(rv["interactiveTxn"], false);
        assert_eq!(rv["sqlFamily"], "sqlite");
        assert_eq!(rv["maxBinds"], D1_MAX_BINDS);
        assert!(d1.meets_host_minimums());
        assert!(DbConnectResult::sqlite().meets_host_minimums());
        assert!(DbConnectResult::postgres().meets_host_minimums());
    }

    #[test]
    fn connect_caps_fail_closed_without_returning_bounds_or_matching_dialect() {
        let mut no_returning = DbConnectResult::sqlite();
        no_returning.returning = false;
        assert!(!no_returning.meets_host_minimums());
        assert!(no_returning
            .capability_failure_reason()
            .contains("returning"));

        let mut zero_rows = DbConnectResult::sqlite();
        zero_rows.max_result_rows = 0;
        assert!(!zero_rows.meets_host_minimums());
        assert!(zero_rows
            .capability_failure_reason()
            .contains("maxResultRows"));

        let mut zero_payload = DbConnectResult::sqlite();
        zero_payload.max_payload_bytes = 0;
        assert!(!zero_payload.meets_host_minimums());
        assert!(zero_payload
            .capability_failure_reason()
            .contains("maxPayloadBytes"));

        let mut zero_result = DbConnectResult::sqlite();
        zero_result.max_result_bytes = 0;
        assert!(!zero_result.meets_host_minimums());
        assert!(zero_result
            .capability_failure_reason()
            .contains("maxResultBytes"));

        let mut zero_cell = DbConnectResult::sqlite();
        zero_cell.max_cell_bytes = 0;
        assert!(!zero_cell.meets_host_minimums());
        assert!(zero_cell
            .capability_failure_reason()
            .contains("maxCellBytes"));

        let mut zero_atomic = DbConnectResult::sqlite();
        zero_atomic.max_atomic_request_bytes = 0;
        assert!(!zero_atomic.meets_host_minimums());
        assert!(zero_atomic
            .capability_failure_reason()
            .contains("maxAtomicRequestBytes"));

        let mut over_scalar = DbConnectResult::sqlite();
        over_scalar.max_atomic_result_bytes = crate::v2::MAX_SCALAR_BYTES + 1;
        assert!(!over_scalar.meets_host_minimums());
        assert!(over_scalar
            .capability_failure_reason()
            .contains("maxAtomicResultBytes"));

        let mut mismatch = DbConnectResult::sqlite();
        mismatch.dialect = "postgres".into();
        assert!(!mismatch.meets_host_minimums());
        assert!(mismatch
            .capability_failure_reason()
            .contains("does not match"));
    }

    #[test]
    fn stmt_exec_result_requires_rows_and_rows_affected() {
        let err = serde_json::from_str::<DbPlanStmtExecResult>(r#"{"rows":[]}"#).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("rowsAffected") || msg.contains("missing field"),
            "{msg}"
        );
        let ok: DbPlanStmtExecResult =
            serde_json::from_str(r#"{"rows":[],"rowsAffected":0}"#).unwrap();
        assert!(ok.rows.is_empty());
        assert_eq!(ok.rows_affected, 0);
    }
}
