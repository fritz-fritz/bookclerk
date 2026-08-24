//! Typed adapter conformance (`ExecuteRequest` / `ExecuteReply`).

use bookclerk_db_exec::{AtomicSession, ExecCaps};
use bookclerk_plugin_abi::{
    DbAtomicRequest, DbConnectResult, DbPlanExecResult, ExecuteRequest,
};

use super::vectors;
use super::SqlFamily;
use sea_orm::DatabaseConnection;

/// Runs the shared contract suite through typed `execute_typed_on_session`.
///
/// # Panics
///
/// Panics when a vector fails.
pub async fn run_typed_conn_vectors(db: &DatabaseConnection, family: SqlFamily, timing: &str) {
    let db = db.clone();
    let timing = timing.to_string();
    vectors::run_contract_vectors(family, vectors::CONTRACT_VECTOR_ROW_CAP, move |req, cap| {
        let db = db.clone();
        let timing = timing.clone();
        async move { run_typed_atomic_request(&db, &req, family, &timing, cap).await }
    })
    .await;
}

/// Executes one legacy vector request through the typed executor and returns plan results.
async fn run_typed_atomic_request(
    db: &DatabaseConnection,
    req: &DbAtomicRequest,
    family: SqlFamily,
    timing: &str,
    cap: u32,
) -> Result<DbPlanExecResult, String> {
    let _plan = req.plan.clone().ok_or_else(|| "vector plan".to_string())?;
    let typed = ExecuteRequest::from_atomic(req).map_err(|err| err.to_string())?;
    let connect = match family {
        SqlFamily::Sqlite => DbConnectResult::sqlite(),
        SqlFamily::Postgres => DbConnectResult::postgres(),
    };
    let mut caps = ExecCaps::from_connect(&connect);
    if cap > 0 {
        caps.max_result_rows = cap;
    }
    let reply = bookclerk_db_exec::execute_typed_on_session(
        db,
        &typed,
        timing,
        caps,
        AtomicSession::from_deadline(None),
    )
    .await
    .map_err(|err| err.to_string())?;
    Ok(reply.into_plan_exec())
}
