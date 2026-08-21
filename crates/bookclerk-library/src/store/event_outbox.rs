//! Durable domain-event outbox and per-subscriber deliveries.

#![allow(clippy::missing_docs_in_private_items)]

use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::anyhow;
use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait,
    ActiveValue::{NotSet, Set},
    ColumnTrait, Condition, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, TransactionTrait,
};
use uuid::Uuid;

use super::{now_str, parse_dt, parse_dt_opt, LibraryStore};
use crate::entities::{books, domain_events, event_deliveries};
use crate::error::{LibraryError, Result};
use crate::models::{
    job_backoff_run_after, AcquireStatus, DomainEventRecord, EventDeliveryFence,
    EventDeliveryRecord, EventSubscriber, PublishDomainEventOutcome, PublishDomainEventSpec,
    EVENT_DELIVERY_MAX_ATTEMPTS,
};

const STATE_PENDING: &str = "pending";
const STATE_DISPATCHED: &str = "dispatched";
const STATE_RUNNING: &str = "running";
const STATE_ACKED: &str = "acked";
const STATE_REJECTED: &str = "rejected";
const STATE_DEAD_LETTER: &str = "dead_letter";

/// Remaining injected `publish_domain_event_on` failures (library tests only).
static INJECT_PUBLISH_FAULTS: AtomicU32 = AtomicU32::new(0);

/// Fail the next `n` outbox inserts (used to prove acquire+publish rollback).
#[cfg(test)]
pub(crate) fn inject_event_publish_failures(n: u32) {
    INJECT_PUBLISH_FAULTS.store(n, Ordering::SeqCst);
}

fn take_publish_fault() -> bool {
    let mut current = INJECT_PUBLISH_FAULTS.load(Ordering::SeqCst);
    loop {
        if current == 0 {
            return false;
        }
        match INJECT_PUBLISH_FAULTS.compare_exchange(
            current,
            current - 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
}

impl LibraryStore {
    /// Persist a domain event. Duplicate `(event_type, dedup_key)` coalesces.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the write fails, or
    /// [`LibraryError::Other`] when the payload exceeds 64 KiB.
    pub async fn publish_domain_event(
        &self,
        spec: PublishDomainEventSpec,
    ) -> Result<PublishDomainEventOutcome> {
        let spec = prepare_publish_domain_event(spec)?;
        if let Some(atomic) = &self.atomic {
            return atomic.publish_domain_event(spec).await;
        }
        let txn = self.db.begin().await.map_err(LibraryError::Orm)?;
        match publish_domain_event_on(&txn, spec).await {
            Ok(outcome) => {
                txn.commit().await.map_err(LibraryError::Orm)?;
                Ok(outcome)
            }
            Err(err) => {
                let _ = txn.rollback().await;
                Err(err)
            }
        }
    }

    /// Snapshot matching subscribers and create one delivery row per plugin.
    ///
    /// Idempotent under replay: existing `(event_id, plugin_id)` rows are kept.
    /// Marks the outbox row `dispatched` in the same transaction.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the write fails.
    pub async fn dispatch_event_deliveries(
        &self,
        event_id: &str,
        subscribers: &[EventSubscriber],
        operation_id: &str,
    ) -> Result<u32> {
        if let Some(atomic) = &self.atomic {
            return atomic
                .dispatch_event_deliveries(event_id, subscribers, operation_id)
                .await;
        }
        let txn = self.db.begin().await.map_err(LibraryError::Orm)?;
        match dispatch_event_deliveries_on(&txn, event_id, subscribers).await {
            Ok(n) => {
                txn.commit().await.map_err(LibraryError::Orm)?;
                Ok(n)
            }
            Err(err) => {
                let _ = txn.rollback().await;
                Err(err)
            }
        }
    }

    /// Next undispatched outbox row (oldest first).
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn next_undispatched_event(&self) -> Result<Option<DomainEventRecord>> {
        let row = domain_events::Entity::find()
            .filter(domain_events::Column::DispatchState.eq(STATE_PENDING))
            .order_by_asc(domain_events::Column::CreatedAt)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        row.map(map_event).transpose()
    }

    /// Claim the next ready delivery with a fenced `pending` → `running` mutation.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the claim write fails.
    pub async fn claim_next_event_delivery(
        &self,
        owner: &str,
        lease_secs: u64,
        operation_id: &str,
        plugin_ids: &[String],
    ) -> Result<Option<EventDeliveryRecord>> {
        if plugin_ids.is_empty() {
            return Ok(None);
        }
        if let Some(atomic) = &self.atomic {
            return atomic
                .claim_next_event_delivery(owner, lease_secs, operation_id, plugin_ids)
                .await;
        }
        let txn = self.db.begin().await.map_err(LibraryError::Orm)?;
        match claim_next_event_delivery_on(&txn, owner, lease_secs, plugin_ids).await {
            Ok(row) => {
                txn.commit().await.map_err(LibraryError::Orm)?;
                Ok(row)
            }
            Err(err) => {
                let _ = txn.rollback().await;
                Err(err)
            }
        }
    }

    /// Return a claimed delivery to `pending` without consuming an attempt.
    ///
    /// Used when this worker cannot execute the subscriber (plugin not loaded).
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn release_unexecuted_event_delivery(
        &self,
        fence: &EventDeliveryFence,
        restore_resume: bool,
    ) -> Result<bool> {
        let now = now_str();
        let mut update = event_deliveries::Entity::update_many()
            .col_expr(
                event_deliveries::Column::State,
                sea_orm::sea_query::Expr::value(STATE_PENDING),
            )
            .col_expr(
                event_deliveries::Column::LeaseOwner,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::LeaseExpiresAt,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now),
            );
        if restore_resume {
            update = update.col_expr(
                event_deliveries::Column::ResumePending,
                sea_orm::sea_query::Expr::value(1i64),
            );
        } else {
            update = update.col_expr(
                event_deliveries::Column::AttemptCount,
                sea_orm::sea_query::Expr::cust(
                    "CASE WHEN attempt_count > 0 THEN attempt_count - 1 ELSE 0 END",
                ),
            );
        }
        let res = update
            .filter(event_deliveries::Column::Id.eq(&fence.delivery_id))
            .filter(event_deliveries::Column::State.eq(STATE_RUNNING))
            .filter(event_deliveries::Column::LeaseOwner.eq(&fence.owner))
            .filter(event_deliveries::Column::LeaseGeneration.eq(fence.generation))
            .exec(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(res.rows_affected == 1)
    }

    /// Refresh the lease when `fence` still owns the running generation.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn heartbeat_event_delivery(
        &self,
        fence: &EventDeliveryFence,
        lease_secs: u64,
    ) -> Result<bool> {
        let now = Utc::now();
        let res = event_deliveries::Entity::update_many()
            .col_expr(
                event_deliveries::Column::LeaseExpiresAt,
                sea_orm::sea_query::Expr::value(Some(
                    (now + Duration::seconds(i64::try_from(lease_secs).unwrap_or(60))).to_rfc3339(),
                )),
            )
            .col_expr(
                event_deliveries::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now.to_rfc3339()),
            )
            .filter(event_deliveries::Column::Id.eq(&fence.delivery_id))
            .filter(event_deliveries::Column::State.eq(STATE_RUNNING))
            .filter(event_deliveries::Column::LeaseOwner.eq(&fence.owner))
            .filter(event_deliveries::Column::LeaseGeneration.eq(fence.generation))
            .exec(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(res.rows_affected == 1)
    }

    /// Mark a fenced delivery `acked`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn ack_event_delivery(&self, fence: &EventDeliveryFence) -> Result<bool> {
        finalize_delivery(&self.db, fence, STATE_ACKED, Some("ack"), None).await
    }

    /// Mark a fenced delivery permanently `rejected`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn reject_event_delivery(
        &self,
        fence: &EventDeliveryFence,
        reason: &str,
    ) -> Result<bool> {
        finalize_delivery(
            &self.db,
            fence,
            STATE_REJECTED,
            Some("reject"),
            Some(reason),
        )
        .await
    }

    /// Mark a fenced delivery `dead_letter`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn dead_letter_event_delivery(
        &self,
        fence: &EventDeliveryFence,
        reason: &str,
    ) -> Result<bool> {
        finalize_delivery(
            &self.db,
            fence,
            STATE_DEAD_LETTER,
            Some("dead_letter"),
            Some(reason),
        )
        .await
    }

    /// Return a fenced delivery to `pending` with backoff (retryable handler).
    ///
    /// When `attempt_count` has already reached `max_attempts`, the delivery is
    /// dead-lettered instead of looping forever on a guest that keeps returning
    /// retry.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn retry_event_delivery(
        &self,
        fence: &EventDeliveryFence,
        run_after: chrono::DateTime<Utc>,
        reason: &str,
    ) -> Result<bool> {
        let Some(row) = event_deliveries::Entity::find_by_id(&fence.delivery_id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(false);
        };
        if row.state != STATE_RUNNING
            || row.lease_owner.as_deref() != Some(fence.owner.as_str())
            || row.lease_generation != fence.generation
        {
            return Ok(false);
        }
        if row.attempt_count >= row.max_attempts {
            return self
                .dead_letter_event_delivery(
                    fence,
                    &format!(
                        "retry exhausted after {} attempts: {reason}",
                        row.attempt_count
                    ),
                )
                .await;
        }
        let now = now_str();
        let res = event_deliveries::Entity::update_many()
            .col_expr(
                event_deliveries::Column::State,
                sea_orm::sea_query::Expr::value(STATE_PENDING),
            )
            .col_expr(
                event_deliveries::Column::RunAfter,
                sea_orm::sea_query::Expr::value(run_after.to_rfc3339()),
            )
            .col_expr(
                event_deliveries::Column::LeaseOwner,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::LeaseExpiresAt,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::ErrorMessage,
                sea_orm::sea_query::Expr::value(Some(reason.to_string())),
            )
            .col_expr(
                event_deliveries::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(event_deliveries::Column::Id.eq(&fence.delivery_id))
            .filter(event_deliveries::Column::State.eq(STATE_RUNNING))
            .filter(event_deliveries::Column::LeaseOwner.eq(&fence.owner))
            .filter(event_deliveries::Column::LeaseGeneration.eq(fence.generation))
            .exec(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(res.rows_affected == 1)
    }

    /// Suspend a fenced delivery (checkpoint + wake) without burning an attempt.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn suspend_event_delivery(
        &self,
        fence: &EventDeliveryFence,
        checkpoint_json: &str,
        checkpoint_schema_version: i64,
        wake_at: chrono::DateTime<Utc>,
    ) -> Result<bool> {
        if checkpoint_json.len() > 65_536 {
            return Err(LibraryError::Other(anyhow::anyhow!(
                "event checkpoint of {} bytes exceeds 65536",
                checkpoint_json.len()
            )));
        }
        let now = Utc::now();
        let Some(row) = event_deliveries::Entity::find_by_id(&fence.delivery_id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(false);
        };
        if row.state != STATE_RUNNING
            || row.lease_owner.as_deref() != Some(fence.owner.as_str())
            || row.lease_generation != fence.generation
        {
            return Ok(false);
        }
        let res = event_deliveries::Entity::update_many()
            .col_expr(
                event_deliveries::Column::State,
                sea_orm::sea_query::Expr::value(STATE_PENDING),
            )
            .col_expr(
                event_deliveries::Column::RunAfter,
                sea_orm::sea_query::Expr::value(wake_at.to_rfc3339()),
            )
            .col_expr(
                event_deliveries::Column::CheckpointJson,
                sea_orm::sea_query::Expr::value(Some(checkpoint_json.to_string())),
            )
            .col_expr(
                event_deliveries::Column::CheckpointSchemaVersion,
                sea_orm::sea_query::Expr::value(checkpoint_schema_version),
            )
            .col_expr(
                event_deliveries::Column::InvocationSequence,
                sea_orm::sea_query::Expr::value(row.invocation_sequence + 1),
            )
            .col_expr(
                event_deliveries::Column::ResumePending,
                sea_orm::sea_query::Expr::value(1i64),
            )
            .col_expr(
                event_deliveries::Column::LeaseOwner,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::LeaseExpiresAt,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now.to_rfc3339()),
            )
            .filter(event_deliveries::Column::Id.eq(&fence.delivery_id))
            .filter(event_deliveries::Column::State.eq(STATE_RUNNING))
            .filter(event_deliveries::Column::LeaseOwner.eq(&fence.owner))
            .filter(event_deliveries::Column::LeaseGeneration.eq(fence.generation))
            .exec(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(res.rows_affected == 1)
    }

    /// Fail a running delivery: retry with backoff or dead-letter at max attempts.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn fail_event_delivery(
        &self,
        fence: &EventDeliveryFence,
        reason: &str,
    ) -> Result<bool> {
        let Some(row) = event_deliveries::Entity::find_by_id(&fence.delivery_id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(false);
        };
        if row.state != STATE_RUNNING
            || row.lease_owner.as_deref() != Some(fence.owner.as_str())
            || row.lease_generation != fence.generation
        {
            return Ok(false);
        }
        if row.attempt_count >= row.max_attempts {
            return self.dead_letter_event_delivery(fence, reason).await;
        }
        let run_after = job_backoff_run_after(row.attempt_count, Utc::now());
        self.retry_event_delivery(fence, run_after, reason).await
    }

    /// Reclaim expired running deliveries back to `pending`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn reclaim_expired_event_deliveries(&self) -> Result<u32> {
        let now = Utc::now();
        let res = event_deliveries::Entity::update_many()
            .col_expr(
                event_deliveries::Column::State,
                sea_orm::sea_query::Expr::value(STATE_PENDING),
            )
            .col_expr(
                event_deliveries::Column::LeaseOwner,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::LeaseExpiresAt,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now.to_rfc3339()),
            )
            .filter(event_deliveries::Column::State.eq(STATE_RUNNING))
            .filter(
                Condition::any()
                    .add(event_deliveries::Column::LeaseExpiresAt.is_null())
                    .add(event_deliveries::Column::LeaseExpiresAt.lte(now.to_rfc3339())),
            )
            .exec(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(u32::try_from(res.rows_affected).unwrap_or(u32::MAX))
    }

    /// Load one event envelope.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn get_domain_event(&self, id: &str) -> Result<Option<DomainEventRecord>> {
        domain_events::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
            .map(map_event)
            .transpose()
    }

    /// Load one delivery.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn get_event_delivery(&self, id: &str) -> Result<Option<EventDeliveryRecord>> {
        event_deliveries::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
            .map(map_delivery)
            .transpose()
    }

    /// Recent outbox events, newest first.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn list_domain_events(&self, limit: u64) -> Result<Vec<DomainEventRecord>> {
        domain_events::Entity::find()
            .order_by_desc(domain_events::Column::CreatedAt)
            .limit(limit)
            .all(&self.db)
            .await
            .map_err(LibraryError::Orm)?
            .into_iter()
            .map(map_event)
            .collect()
    }

    /// Deliveries filtered by optional state, newest first.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn list_event_deliveries(
        &self,
        state: Option<&str>,
        limit: u64,
    ) -> Result<Vec<EventDeliveryRecord>> {
        let mut q = event_deliveries::Entity::find();
        if let Some(state) = state {
            q = q.filter(event_deliveries::Column::State.eq(state));
        }
        q.order_by_desc(event_deliveries::Column::UpdatedAt)
            .limit(limit)
            .all(&self.db)
            .await
            .map_err(LibraryError::Orm)?
            .into_iter()
            .map(map_delivery)
            .collect()
    }

    /// Re-queue a dead-lettered delivery for operator retry.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn retry_dead_letter_delivery(&self, id: &str) -> Result<bool> {
        let now = now_str();
        let res = event_deliveries::Entity::update_many()
            .col_expr(
                event_deliveries::Column::State,
                sea_orm::sea_query::Expr::value(STATE_PENDING),
            )
            .col_expr(
                event_deliveries::Column::RunAfter,
                sea_orm::sea_query::Expr::value(now.clone()),
            )
            .col_expr(
                event_deliveries::Column::Outcome,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::ErrorMessage,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::AttemptCount,
                sea_orm::sea_query::Expr::value(0i64),
            )
            .col_expr(
                event_deliveries::Column::ResumePending,
                sea_orm::sea_query::Expr::value(0i64),
            )
            .col_expr(
                event_deliveries::Column::InvocationSequence,
                sea_orm::sea_query::Expr::value(0i64),
            )
            .col_expr(
                event_deliveries::Column::CheckpointJson,
                sea_orm::sea_query::Expr::value(Option::<String>::None),
            )
            .col_expr(
                event_deliveries::Column::CheckpointSchemaVersion,
                sea_orm::sea_query::Expr::value(0i64),
            )
            .col_expr(
                event_deliveries::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(event_deliveries::Column::Id.eq(id))
            .filter(event_deliveries::Column::State.eq(STATE_DEAD_LETTER))
            .exec(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(res.rows_affected == 1)
    }

    /// Operator acknowledge: mark a dead-letter as rejected (no further retry).
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn acknowledge_dead_letter_delivery(&self, id: &str) -> Result<bool> {
        let now = now_str();
        let res = event_deliveries::Entity::update_many()
            .col_expr(
                event_deliveries::Column::State,
                sea_orm::sea_query::Expr::value(STATE_REJECTED),
            )
            .col_expr(
                event_deliveries::Column::Outcome,
                sea_orm::sea_query::Expr::value(Some("ack".to_string())),
            )
            .col_expr(
                event_deliveries::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(event_deliveries::Column::Id.eq(id))
            .filter(event_deliveries::Column::State.eq(STATE_DEAD_LETTER))
            .exec(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(res.rows_affected == 1)
    }

    /// Count deliveries in `state`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn count_event_deliveries(&self, state: &str) -> Result<i64> {
        let n = event_deliveries::Entity::find()
            .filter(event_deliveries::Column::State.eq(state))
            .count(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        i64::try_from(n).map_err(|err| LibraryError::Other(anyhow::anyhow!(err.to_string())))
    }
}

/// Mint an event id when empty and reject oversized or blank publish fields.
///
/// # Errors
///
/// Returns [`LibraryError::Other`] when the payload exceeds 64 KiB or when
/// `event_type` / `dedup_key` are blank.
pub fn prepare_publish_domain_event(
    mut spec: PublishDomainEventSpec,
) -> Result<PublishDomainEventSpec> {
    const MAX_PAYLOAD: usize = 65_536;
    if spec.payload.len() > MAX_PAYLOAD {
        return Err(LibraryError::Other(anyhow::anyhow!(
            "domain event payload of {} bytes exceeds {MAX_PAYLOAD}",
            spec.payload.len()
        )));
    }
    if spec.event_type.trim().is_empty() || spec.dedup_key.trim().is_empty() {
        return Err(LibraryError::Other(anyhow::anyhow!(
            "domain event type and dedup_key are required"
        )));
    }
    if spec.id.trim().is_empty() {
        spec.id = Uuid::new_v4().to_string();
    } else {
        spec.id = spec.id.trim().to_string();
    }
    Ok(spec)
}

/// Build the `book_acquired` outbox spec for a library row.
pub(crate) fn book_acquired_spec(
    book: &books::Model,
    storage_key: Option<&str>,
) -> PublishDomainEventSpec {
    let key = storage_key
        .map(str::to_string)
        .or_else(|| book.storage_key.clone())
        .unwrap_or_default();
    let path_keys = if key.is_empty() {
        Vec::<String>::new()
    } else {
        vec![key]
    };
    let payload = serde_json::json!({
        "titleId": book.uuid,
        "source": book.source,
        "asin": book.asin,
        "isbn": book.isbn,
        "pathKeys": path_keys,
        "accountId": book.account_id,
    });
    PublishDomainEventSpec {
        id: String::new(),
        event_type: "book_acquired".into(),
        schema_version: 1,
        account_id: book.account_id.clone(),
        correlation_id: book.uuid.clone(),
        causation_id: String::new(),
        dedup_key: format!("book_acquired:{}", book.uuid),
        payload: serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into()),
        ordering_key: book.uuid.clone(),
    }
}

/// Apply acquire status and, when becoming acquired, publish the outbox row.
pub(crate) async fn set_acquire_status_on<C: ConnectionTrait>(
    db: &C,
    model: books::Model,
    status: AcquireStatus,
    storage_key: Option<&str>,
    error_message: Option<&str>,
) -> Result<()> {
    let spec = if status == AcquireStatus::Acquired {
        Some(prepare_publish_domain_event(book_acquired_spec(
            &model,
            storage_key,
        ))?)
    } else {
        None
    };
    update_acquire_status_on(db, model, status, storage_key, error_message).await?;
    if let Some(spec) = spec {
        publish_domain_event_on(db, spec).await?;
    }
    Ok(())
}

/// Update acquire columns without publishing.
pub(crate) async fn update_acquire_status_on<C: ConnectionTrait>(
    db: &C,
    model: books::Model,
    status: AcquireStatus,
    storage_key: Option<&str>,
    error_message: Option<&str>,
) -> Result<()> {
    let mut am: books::ActiveModel = model.into();
    am.acquire_status = Set(status.as_str().to_string());
    am.storage_key = Set(storage_key.map(str::to_string));
    am.error_message = Set(error_message.map(str::to_string));
    am.updated_at = Set(now_str());
    am.update(db).await.map_err(LibraryError::Orm)?;
    Ok(())
}

/// Insert an outbox row or return the existing id for the same dedup key.
pub(crate) async fn publish_domain_event_on<C: ConnectionTrait>(
    db: &C,
    spec: PublishDomainEventSpec,
) -> Result<PublishDomainEventOutcome> {
    if take_publish_fault() {
        return Err(LibraryError::Other(anyhow!("injected event publish fault")));
    }
    let spec = prepare_publish_domain_event(spec)?;
    if let Some(existing) = domain_events::Entity::find()
        .filter(domain_events::Column::EventType.eq(&spec.event_type))
        .filter(domain_events::Column::DedupKey.eq(&spec.dedup_key))
        .one(db)
        .await
        .map_err(LibraryError::Orm)?
    {
        return Ok(PublishDomainEventOutcome::Duplicate {
            existing_id: existing.id,
        });
    }
    let now = now_str();
    let id = spec.id;
    let event_type = spec.event_type.clone();
    let dedup_key = spec.dedup_key.clone();
    let model = domain_events::ActiveModel {
        id: Set(id.clone()),
        event_type: Set(spec.event_type),
        schema_version: Set(spec.schema_version.max(1)),
        occurred_at: Set(now.clone()),
        account_id: Set(spec.account_id),
        correlation_id: Set(spec.correlation_id),
        causation_id: Set(spec.causation_id),
        dedup_key: Set(spec.dedup_key),
        payload: Set(spec.payload),
        ordering_key: Set(spec.ordering_key),
        dispatch_state: Set(STATE_PENDING.into()),
        created_at: Set(now),
    };
    match model.insert(db).await {
        Ok(_) => Ok(PublishDomainEventOutcome::Created { id }),
        Err(err) if is_unique_violation(&err) => {
            if let Some(existing) = domain_events::Entity::find()
                .filter(domain_events::Column::EventType.eq(event_type))
                .filter(domain_events::Column::DedupKey.eq(dedup_key))
                .one(db)
                .await
                .map_err(LibraryError::Orm)?
            {
                return Ok(PublishDomainEventOutcome::Duplicate {
                    existing_id: existing.id,
                });
            }
            Err(LibraryError::Orm(err))
        }
        Err(err) => Err(LibraryError::Orm(err)),
    }
}

/// Create deliveries for `subscribers` and mark the event dispatched.
pub(crate) async fn dispatch_event_deliveries_on<C: ConnectionTrait>(
    db: &C,
    event_id: &str,
    subscribers: &[EventSubscriber],
) -> Result<u32> {
    let Some(event) = domain_events::Entity::find_by_id(event_id)
        .one(db)
        .await
        .map_err(LibraryError::Orm)?
    else {
        return Ok(0);
    };
    let now = now_str();
    let mut created = 0u32;
    for sub in subscribers {
        if sub.plugin_id.trim().is_empty() {
            continue;
        }
        let delivery_id = format!("{event_id}:{}", sub.plugin_id);
        let idempotency_key = delivery_id.clone();
        let model = event_deliveries::ActiveModel {
            id: Set(delivery_id),
            event_id: Set(event_id.to_string()),
            plugin_id: Set(sub.plugin_id.clone()),
            idempotency_key: Set(idempotency_key),
            state: Set(STATE_PENDING.into()),
            attempt_count: Set(0),
            max_attempts: Set(EVENT_DELIVERY_MAX_ATTEMPTS),
            lease_owner: NotSet,
            lease_expires_at: NotSet,
            lease_generation: Set(0),
            run_after: Set(now.clone()),
            invocation_sequence: Set(0),
            resume_pending: Set(0),
            checkpoint_json: NotSet,
            checkpoint_schema_version: Set(0),
            ordering_key: Set(event.ordering_key.clone()),
            outcome: NotSet,
            error_message: NotSet,
            created_at: Set(now.clone()),
            updated_at: Set(now.clone()),
        };
        match model.insert(db).await {
            Ok(_) => created += 1,
            Err(err) if is_unique_violation(&err) => {}
            Err(err) => return Err(LibraryError::Orm(err)),
        }
    }
    domain_events::Entity::update_many()
        .col_expr(
            domain_events::Column::DispatchState,
            sea_orm::sea_query::Expr::value(STATE_DISPATCHED),
        )
        .filter(domain_events::Column::Id.eq(event_id))
        .filter(domain_events::Column::DispatchState.eq(STATE_PENDING))
        .exec(db)
        .await
        .map_err(LibraryError::Orm)?;
    Ok(created)
}

/// Claim the next ready delivery, skipping blocked FIFO keys.
pub(crate) async fn claim_next_event_delivery_on<C: ConnectionTrait>(
    db: &C,
    owner: &str,
    lease_secs: u64,
    plugin_ids: &[String],
) -> Result<Option<EventDeliveryRecord>> {
    if plugin_ids.is_empty() {
        return Ok(None);
    }
    const PAGE: u64 = 64;
    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let lease_expires =
        (now + Duration::seconds(i64::try_from(lease_secs).unwrap_or(60))).to_rfc3339();
    let mut offset = 0u64;
    loop {
        let candidates = event_deliveries::Entity::find()
            .filter(event_deliveries::Column::State.eq(STATE_PENDING))
            .filter(event_deliveries::Column::RunAfter.lte(now_s.clone()))
            .filter(event_deliveries::Column::PluginId.is_in(plugin_ids.to_vec()))
            .order_by_asc(event_deliveries::Column::CreatedAt)
            .limit(PAGE)
            .offset(offset)
            .all(db)
            .await
            .map_err(LibraryError::Orm)?;
        if candidates.is_empty() {
            return Ok(None);
        }
        let page_len = u64::try_from(candidates.len()).unwrap_or(PAGE);
        for model in candidates {
            if fifo_blocked(db, &model).await? {
                continue;
            }
            let resuming = model.resume_pending != 0;
            let attempt = if resuming {
                model.attempt_count.max(1)
            } else {
                model.attempt_count + 1
            };
            let generation = model.lease_generation + 1;
            let mut update = event_deliveries::Entity::update_many()
                .col_expr(
                    event_deliveries::Column::State,
                    sea_orm::sea_query::Expr::value(STATE_RUNNING),
                )
                .col_expr(
                    event_deliveries::Column::AttemptCount,
                    sea_orm::sea_query::Expr::value(attempt),
                )
                .col_expr(
                    event_deliveries::Column::LeaseOwner,
                    sea_orm::sea_query::Expr::value(Some(owner.to_string())),
                )
                .col_expr(
                    event_deliveries::Column::LeaseExpiresAt,
                    sea_orm::sea_query::Expr::value(Some(lease_expires.clone())),
                )
                .col_expr(
                    event_deliveries::Column::LeaseGeneration,
                    sea_orm::sea_query::Expr::value(generation),
                )
                .col_expr(
                    event_deliveries::Column::UpdatedAt,
                    sea_orm::sea_query::Expr::value(now_s.clone()),
                );
            if resuming {
                update = update.col_expr(
                    event_deliveries::Column::ResumePending,
                    sea_orm::sea_query::Expr::value(0i64),
                );
            }
            let res = update
                .filter(event_deliveries::Column::Id.eq(&model.id))
                .filter(event_deliveries::Column::State.eq(STATE_PENDING))
                .exec(db)
                .await
                .map_err(LibraryError::Orm)?;
            if res.rows_affected != 1 {
                continue;
            }
            let Some(updated) = event_deliveries::Entity::find_by_id(&model.id)
                .one(db)
                .await
                .map_err(LibraryError::Orm)?
            else {
                continue;
            };
            return Ok(Some(map_delivery(updated)?));
        }
        if page_len < PAGE {
            return Ok(None);
        }
        offset += PAGE;
    }
}

async fn fifo_blocked<C: ConnectionTrait>(
    db: &C,
    candidate: &event_deliveries::Model,
) -> Result<bool> {
    if candidate.ordering_key.is_empty() {
        return Ok(false);
    }
    let n = event_deliveries::Entity::find()
        .filter(event_deliveries::Column::PluginId.eq(&candidate.plugin_id))
        .filter(event_deliveries::Column::OrderingKey.eq(&candidate.ordering_key))
        .filter(event_deliveries::Column::CreatedAt.lt(&candidate.created_at))
        .filter(event_deliveries::Column::State.is_in([STATE_PENDING, STATE_RUNNING]))
        .count(db)
        .await
        .map_err(LibraryError::Orm)?;
    Ok(n > 0)
}

async fn finalize_delivery<C: ConnectionTrait>(
    db: &C,
    fence: &EventDeliveryFence,
    state: &str,
    outcome: Option<&str>,
    error: Option<&str>,
) -> Result<bool> {
    let now = now_str();
    let res = event_deliveries::Entity::update_many()
        .col_expr(
            event_deliveries::Column::State,
            sea_orm::sea_query::Expr::value(state),
        )
        .col_expr(
            event_deliveries::Column::Outcome,
            sea_orm::sea_query::Expr::value(outcome.map(str::to_string)),
        )
        .col_expr(
            event_deliveries::Column::ErrorMessage,
            sea_orm::sea_query::Expr::value(error.map(str::to_string)),
        )
        .col_expr(
            event_deliveries::Column::LeaseOwner,
            sea_orm::sea_query::Expr::value(Option::<String>::None),
        )
        .col_expr(
            event_deliveries::Column::LeaseExpiresAt,
            sea_orm::sea_query::Expr::value(Option::<String>::None),
        )
        .col_expr(
            event_deliveries::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(event_deliveries::Column::Id.eq(&fence.delivery_id))
        .filter(event_deliveries::Column::State.eq(STATE_RUNNING))
        .filter(event_deliveries::Column::LeaseOwner.eq(&fence.owner))
        .filter(event_deliveries::Column::LeaseGeneration.eq(fence.generation))
        .exec(db)
        .await
        .map_err(LibraryError::Orm)?;
    Ok(res.rows_affected == 1)
}

fn is_unique_violation(err: &sea_orm::DbErr) -> bool {
    let s = err.to_string().to_lowercase();
    s.contains("unique") || s.contains("constraint")
}

fn map_event(m: domain_events::Model) -> Result<DomainEventRecord> {
    Ok(DomainEventRecord {
        id: m.id,
        event_type: m.event_type,
        schema_version: m.schema_version,
        occurred_at: parse_dt(&m.occurred_at),
        account_id: m.account_id,
        correlation_id: m.correlation_id,
        causation_id: m.causation_id,
        dedup_key: m.dedup_key,
        payload: m.payload,
        ordering_key: m.ordering_key,
        dispatch_state: m.dispatch_state,
        created_at: parse_dt(&m.created_at),
    })
}

fn map_delivery(m: event_deliveries::Model) -> Result<EventDeliveryRecord> {
    Ok(EventDeliveryRecord {
        id: m.id,
        event_id: m.event_id,
        plugin_id: m.plugin_id,
        idempotency_key: m.idempotency_key,
        state: m.state,
        attempt_count: m.attempt_count,
        max_attempts: m.max_attempts,
        lease_owner: m.lease_owner,
        lease_expires_at: parse_dt_opt(m.lease_expires_at.as_deref()),
        lease_generation: m.lease_generation,
        run_after: parse_dt(&m.run_after),
        invocation_sequence: m.invocation_sequence,
        resume_pending: m.resume_pending != 0,
        checkpoint_json: m.checkpoint_json,
        checkpoint_schema_version: m.checkpoint_schema_version,
        ordering_key: m.ordering_key,
        outcome: m.outcome,
        error_message: m.error_message,
        created_at: parse_dt(&m.created_at),
        updated_at: parse_dt(&m.updated_at),
    })
}
