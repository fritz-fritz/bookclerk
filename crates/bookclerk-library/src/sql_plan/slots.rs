//! Portable COUNT+mutate serialization via `db_serialization_slots`.

use sea_orm::ConnectionTrait;

use crate::error::{LibraryError, Result};

/// Slot key for job admit / claim / scratch-quota serialization.
pub const JOB_QUEUE_SLOT: &str = "job-queue";

/// Slot key that serializes in-flight event deliveries for one plugin class.
#[must_use]
pub fn event_inflight_slot(plugin_id: &str, resource_class: &str) -> String {
    let class = if resource_class.trim().is_empty() {
        crate::EVENT_RESOURCE_CLASS_NETWORK
    } else {
        resource_class.trim()
    };
    format!("event-inflight:{plugin_id}:{class}")
}

/// Inserts the slot row if missing, then bumps it (transaction-scoped write lock).
///
/// Canonical SQL uses `?` placeholders. Host transport never lowers; adapters
/// realize `INSERT OR IGNORE` at the execute edge. This helper must run on the
/// caller's transaction, not a nested `BEGIN`.
///
/// # Errors
///
/// Returns [`LibraryError::Orm`] when either statement fails.
pub async fn lock_serialization_slot<C: ConnectionTrait>(db: &C, slot_key: &str) -> Result<()> {
    const INSERT: &str = "INSERT OR IGNORE INTO db_serialization_slots (slot_key, bump) \
         SELECT ?, 0 WHERE NOT EXISTS (\
            SELECT 1 FROM db_serialization_slots WHERE slot_key = ?\
         )";
    const BUMP: &str = "UPDATE db_serialization_slots SET bump = bump + 1 WHERE slot_key = ?";
    crate::host_sql::execute_host_canonical(db, INSERT, [slot_key.into(), slot_key.into()])
        .await
        .map_err(LibraryError::Orm)?;
    crate::host_sql::execute_host_canonical(db, BUMP, [slot_key.into()])
        .await
        .map_err(LibraryError::Orm)?;
    Ok(())
}
