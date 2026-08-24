//! Adapter-boundary typed conformance for the platform SQLite guest.

use bookclerk_plugin_sdk::database_adapter::guest_execute_atomic;
use bookclerk_plugin_sdk::{GuestReceiptPersist, HostExecuteEnvelope};

#[tokio::test]
async fn platform_sqlite_guest_execute_passes_typed_vectors() {
    let db = bookclerk_plugin_database_sqlite::open_memory()
        .await
        .expect("mem db");
    bookclerk_plugin_sdk::database_adapter::set_connection(db).await;
    bookclerk_library::sql_plan::run_typed_request_vectors(
        bookclerk_plugin_sdk::DbConnectResult::sqlite(),
        bookclerk_plugin_sdk::DbConnectResult::sqlite().max_result_rows,
        |req| async move {
            guest_execute_atomic(HostExecuteEnvelope::new(
                req,
                GuestReceiptPersist::default(),
            ))
            .await
            .map_err(|err| err.to_string())
        },
    )
    .await;
}
