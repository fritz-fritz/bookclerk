//! Hidden JSON / sentinel compatibility surface for first-party database guests.
//!
//! Third-party plugin authors should use typed [`ExecuteRequest`] /
//! [`DatabaseBinding`](crate::DatabaseBinding) instead. These items remain
//! available for in-tree adapters migrating off `bookclerk.atomic` sentinels.

#![doc(hidden)]

pub use bookclerk_plugin_abi::{
    sea_null, sea_null_kind, DbAtomicPlan, DbAtomicRequest, DbAtomicTiming, DbBeginParams,
    DbBeginResult, DbConnectParams, DbConnectResult, DbPlanExecResult, DbPlanStatement,
    DbPlanStatementKind, DbPlanStmtExecResult, DbTxnParams, ExecResultDto, ProxyRowDto,
    QueryResultDto, StatementDto, D1_MAX_BINDS, DB_ATOMIC_SENTINEL, DB_CAPABILITIES_SENTINEL,
    SEA_NULL_KEY,
};

pub use crate::db::{
    b64_string_to_bytes, bytes_to_b64_string, db_value_from_sea, db_value_to_sea,
    exec_result_from_dto, exec_result_to_dto, json_to_sea_value, proxy_rows_from_dto,
    proxy_rows_from_typed, proxy_rows_to_dto, sea_value_to_json, statement_from_dto,
    statement_to_dto,
};
