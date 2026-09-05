//! Adapter-boundary typed conformance for the platform SQLite guest.

use bookclerk_db_guest::{guest_execute_request, set_connection};
use bookclerk_plugin_abi::{apply_schema_sql_to_env, DbCapabilities};

#[tokio::test]
async fn platform_sqlite_guest_execute_passes_typed_vectors() {
    let db = bookclerk_plugin_database_sqlite::open_memory()
        .await
        .expect("mem db");
    set_connection(db).await;
    let mut catalog = bookclerk_library::migrations::host_sql_type_env();
    bookclerk_library::sql_plan::run_typed_request_vectors(
        DbCapabilities::advertised_sqlite(),
        DbCapabilities::advertised_sqlite().max_result_rows,
        |req| {
            let mut req = req;
            bookclerk_plugin_abi::desugar_execute_request(&mut req);
            let envelope = bookclerk_db_exec::stamp_adapter_execute(req.clone(), &catalog);
            if envelope.is_ok() {
                for stmt in &req.statements {
                    apply_schema_sql_to_env(&mut catalog, &stmt.sql);
                }
            }
            async move {
                guest_execute_request(envelope.map_err(|err| err.to_string())?)
                    .await
                    .map_err(|err| err.to_string())
            }
        },
    )
    .await;
}
