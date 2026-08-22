//! Portable COUNT+mutate serialization via `db_serialization_slots`.

use sea_orm::{ConnectionTrait, Statement};

use super::dialect::{render_statement, SqlFamily};
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
/// # Errors
///
/// Returns [`LibraryError::Orm`] when either statement fails.
pub async fn lock_serialization_slot<C: ConnectionTrait>(db: &C, slot_key: &str) -> Result<()> {
    let family = SqlFamily::from_sea(db.get_database_backend());
    let backend = family.sea_backend();
    let insert = render_statement(
        family,
        "INSERT OR IGNORE INTO db_serialization_slots (slot_key, bump) \
         SELECT ?, 0 WHERE NOT EXISTS (\
            SELECT 1 FROM db_serialization_slots WHERE slot_key = ?\
         )",
    );
    let bump = render_statement(
        family,
        "UPDATE db_serialization_slots SET bump = bump + 1 WHERE slot_key = ?",
    );
    db.execute_raw(Statement::from_sql_and_values(
        backend,
        &insert,
        [slot_key.into(), slot_key.into()],
    ))
    .await
    .map_err(LibraryError::Orm)?;
    db.execute_raw(Statement::from_sql_and_values(
        backend,
        &bump,
        [slot_key.into()],
    ))
    .await
    .map_err(LibraryError::Orm)?;
    Ok(())
}
