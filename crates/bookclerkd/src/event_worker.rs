//! Durable domain-event dispatcher and fenced delivery worker.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bookclerk_integrations::{DomainEvent, EventResult};
use bookclerk_library::{EventSubscriber, LibraryStore};
use chrono::{TimeZone, Utc};
use tracing::{info, warn};
use uuid::Uuid;

use crate::api::AppState;

/// How long a worker may replay a lost `claim_next_event_delivery` RPC.
const CLAIM_REPLAY_BUDGET: Duration = Duration::from_secs(2);
/// Delay between bounded delivery-claim replay attempts.
const CLAIM_REPLAY_DELAY: Duration = Duration::from_millis(200);
/// Lease duration granted to a delivery worker on claim.
const LEASE_SECS: u64 = 60;

/// Dispatch undispatched outbox rows and run one delivery worker.
pub fn start_event_runtime(state: Arc<AppState>) {
    info!("starting durable event dispatcher");
    spawn_dispatcher(state.clone());
    spawn_delivery_worker(state);
}

/// Tick the outbox dispatcher on notify and a 5s idle interval.
fn spawn_dispatcher(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut idle = tokio::time::interval(Duration::from_secs(5));
        idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let library = state.library_snapshot().await;
            if let Err(err) = dispatch_pending(&state, &library).await {
                warn!(error = %err, "event dispatch failed");
            }
            tokio::select! {
                () = state.job_notify.notified() => {}
                _ = idle.tick() => {}
            }
        }
    });
}

/// Claim and run deliveries, reclaiming expired leases between idle waits.
fn spawn_delivery_worker(state: Arc<AppState>) {
    tokio::spawn(async move {
        let owner = format!("event-{}", Uuid::new_v4());
        let mut idle = tokio::time::interval(Duration::from_secs(5));
        idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let library = state.library_snapshot().await;
            if let Err(err) = library.reclaim_expired_event_deliveries().await {
                warn!(error = %err, "event delivery reclaim failed");
            }
            match claim_delivery(&library, &owner).await {
                Ok(Some(delivery)) => {
                    run_delivery(&state, &library, delivery).await;
                }
                Ok(None) => {
                    tokio::select! {
                        () = state.job_notify.notified() => {}
                        _ = idle.tick() => {}
                    }
                }
                Err(err) => {
                    warn!(error = %err, "claim_next_event_delivery failed");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
}

/// Create deliveries for undispatched outbox rows using currently loaded subscribers.
async fn dispatch_pending(
    state: &AppState,
    library: &LibraryStore,
) -> Result<(), bookclerk_library::LibraryError> {
    loop {
        let Some(event) = library.next_undispatched_event().await? else {
            return Ok(());
        };
        let subscribers = {
            let integrations = state.integrations.read().await;
            integrations
                .all()
                .iter()
                .filter(|i| {
                    i.event_subscriptions().iter().any(|s| {
                        s.event_type == event.event_type
                            && s.schema_versions
                                .iter()
                                .any(|v| i64::from(*v) == event.schema_version)
                    })
                })
                .map(|i| EventSubscriber {
                    plugin_id: i.id().to_string(),
                })
                .collect::<Vec<_>>()
        };
        let op = format!("dispatch-{}", event.id);
        let n = library
            .dispatch_event_deliveries(&event.id, &subscribers, &op)
            .await?;
        info!(
            event_id = %event.id,
            event_type = %event.event_type,
            deliveries = n,
            "dispatched domain event"
        );
        state.job_notify.notify_waiters();
    }
}

/// Claim the next ready delivery, replaying a lost RPC within [`CLAIM_REPLAY_BUDGET`].
async fn claim_delivery(
    library: &LibraryStore,
    owner: &str,
) -> Result<Option<bookclerk_library::EventDeliveryRecord>, bookclerk_library::LibraryError> {
    let operation_id = Uuid::new_v4().to_string();
    let deadline = Instant::now() + CLAIM_REPLAY_BUDGET;
    loop {
        match library
            .claim_next_event_delivery(owner, LEASE_SECS, &operation_id)
            .await
        {
            Ok(row) => return Ok(row),
            Err(bookclerk_library::LibraryError::Unavailable(msg)) => {
                if Instant::now() >= deadline {
                    return Err(bookclerk_library::LibraryError::Unavailable(format!(
                        "claim replay budget exhausted: {msg}"
                    )));
                }
                tokio::time::sleep(CLAIM_REPLAY_DELAY).await;
            }
            Err(err) => return Err(err),
        }
    }
}

/// Invoke `Integration.onEvent` and persist the fenced [`EventResult`].
async fn run_delivery(
    state: &AppState,
    library: &LibraryStore,
    delivery: bookclerk_library::EventDeliveryRecord,
) {
    let fence = delivery.fence();
    let Some(event) = library
        .get_domain_event(&delivery.event_id)
        .await
        .ok()
        .flatten()
    else {
        let _ = library
            .dead_letter_event_delivery(&fence, "parent event missing")
            .await;
        return;
    };
    let integration = {
        let integrations = state.integrations.read().await;
        integrations.get(&delivery.plugin_id)
    };
    let Some(integration) = integration else {
        let _ = library
            .retry_event_delivery(
                &fence,
                Utc::now() + chrono::Duration::seconds(30),
                "subscriber not loaded",
            )
            .await;
        return;
    };
    let domain = domain_event_for_delivery(&event, &delivery);
    let result = match integration.deliver_domain_event(domain).await {
        Ok(result) => result,
        Err(err) => {
            warn!(
                delivery_id = %delivery.id,
                plugin = %delivery.plugin_id,
                error = %err,
                "event delivery invocation failed"
            );
            let _ = library.fail_event_delivery(&fence, &err.to_string()).await;
            return;
        }
    };
    let still_owned = library
        .heartbeat_event_delivery(&fence, LEASE_SECS)
        .await
        .unwrap_or(false);
    if !still_owned {
        warn!(
            delivery_id = %delivery.id,
            "event delivery fence lost; ignoring handler result"
        );
        return;
    }
    let persist = match result {
        EventResult::Ack => {
            info!(
                delivery_id = %delivery.id,
                plugin = %delivery.plugin_id,
                outcome = "ack",
                "event delivery completed"
            );
            library.ack_event_delivery(&fence).await
        }
        EventResult::Retry {
            retry_at_unix_ms,
            reason,
        } => {
            let wake = unix_ms_to_utc(retry_at_unix_ms)
                .unwrap_or_else(|| Utc::now() + chrono::Duration::seconds(5));
            if delivery.attempt_count >= delivery.max_attempts {
                warn!(
                    delivery_id = %delivery.id,
                    plugin = %delivery.plugin_id,
                    outcome = "dead_letter",
                    %reason,
                    attempts = delivery.attempt_count,
                    "event delivery retry exhausted"
                );
            } else {
                info!(
                    delivery_id = %delivery.id,
                    plugin = %delivery.plugin_id,
                    outcome = "retry",
                    %reason,
                    "event delivery retry scheduled"
                );
            }
            library.retry_event_delivery(&fence, wake, &reason).await
        }
        EventResult::Reject { reason } => {
            warn!(
                delivery_id = %delivery.id,
                plugin = %delivery.plugin_id,
                outcome = "reject",
                %reason,
                "event delivery rejected"
            );
            library.reject_event_delivery(&fence, &reason).await
        }
        EventResult::DeadLetter { reason } => {
            warn!(
                delivery_id = %delivery.id,
                plugin = %delivery.plugin_id,
                outcome = "dead_letter",
                %reason,
                "event delivery dead-lettered"
            );
            library.dead_letter_event_delivery(&fence, &reason).await
        }
        EventResult::Suspended {
            checkpoint_json,
            checkpoint_schema_version,
            wake_at_unix_ms,
        } => {
            let allowed = integration
                .event_subscriptions()
                .iter()
                .any(|s| s.event_type == event.event_type && s.supports_suspend);
            if !allowed {
                warn!(
                    delivery_id = %delivery.id,
                    plugin = %delivery.plugin_id,
                    "event suspend not advertised; treating as reject"
                );
                library
                    .reject_event_delivery(&fence, "suspend not advertised for this event type")
                    .await
            } else {
                let wake = unix_ms_to_utc(wake_at_unix_ms)
                    .unwrap_or_else(|| Utc::now() + chrono::Duration::seconds(30));
                info!(
                    delivery_id = %delivery.id,
                    plugin = %delivery.plugin_id,
                    outcome = "suspended",
                    "event delivery suspended"
                );
                library
                    .suspend_event_delivery(
                        &fence,
                        &checkpoint_json,
                        i64::from(checkpoint_schema_version),
                        wake,
                    )
                    .await
            }
        }
    };
    if let Err(err) = persist {
        warn!(delivery_id = %delivery.id, error = %err, "event delivery persist failed");
    }
}

/// Parse a unix-ms timestamp into UTC, ignoring overflows.
fn unix_ms_to_utc(ms: u64) -> Option<chrono::DateTime<Utc>> {
    let ms = i64::try_from(ms).ok()?;
    Utc.timestamp_millis_opt(ms).single()
}

/// Map an outbox envelope plus the claimed delivery onto an ABI [`DomainEvent`].
///
/// Claim clears `resume_pending` on the row; a stored checkpoint still means
/// this invocation continues a prior [`EventResult::Suspended`].
fn domain_event_for_delivery(
    event: &bookclerk_library::DomainEventRecord,
    delivery: &bookclerk_library::EventDeliveryRecord,
) -> DomainEvent {
    DomainEvent {
        event_id: event.id.clone(),
        event_type: event.event_type.clone(),
        schema_version: u32::try_from(event.schema_version).unwrap_or(1),
        occurred_at_unix_ms: u64::try_from(event.occurred_at.timestamp_millis()).unwrap_or(0),
        account_id: event.account_id.clone(),
        correlation_id: event.correlation_id.clone(),
        causation_id: event.causation_id.clone(),
        deduplication_key: delivery.idempotency_key.clone(),
        delivery_attempt: u32::try_from(delivery.attempt_count.max(1)).unwrap_or(1),
        payload: event.payload.as_bytes().to_vec(),
        checkpoint_json: delivery.checkpoint_json.clone().unwrap_or_default(),
        checkpoint_schema_version: u32::try_from(delivery.checkpoint_schema_version).unwrap_or(0),
        invocation_sequence: u32::try_from(delivery.invocation_sequence).unwrap_or(0),
        resume_pending: delivery.checkpoint_json.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookclerk_library::{DomainEventRecord, EventDeliveryRecord};
    use chrono::TimeZone;

    fn sample_event() -> DomainEventRecord {
        DomainEventRecord {
            id: "evt-1".into(),
            event_type: "book_acquired".into(),
            schema_version: 1,
            occurred_at: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            account_id: "acct".into(),
            correlation_id: "corr".into(),
            causation_id: String::new(),
            dedup_key: "book_acquired:u1".into(),
            payload: r#"{"titleId":"u1"}"#.into(),
            dispatch_state: "dispatched".into(),
            created_at: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
        }
    }

    fn sample_delivery() -> EventDeliveryRecord {
        EventDeliveryRecord {
            id: "evt-1:echo".into(),
            event_id: "evt-1".into(),
            plugin_id: "echo".into(),
            idempotency_key: "evt-1:echo".into(),
            state: "running".into(),
            attempt_count: 1,
            max_attempts: 8,
            lease_owner: Some("worker".into()),
            lease_expires_at: None,
            lease_generation: 1,
            run_after: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            invocation_sequence: 0,
            resume_pending: false,
            checkpoint_json: None,
            checkpoint_schema_version: 0,
            ordering_key: "u1".into(),
            outcome: None,
            error_message: None,
            created_at: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            updated_at: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
        }
    }

    #[test]
    fn first_delivery_has_empty_checkpoint() {
        let domain = domain_event_for_delivery(&sample_event(), &sample_delivery());
        assert!(domain.checkpoint_json.is_empty());
        assert_eq!(domain.checkpoint_schema_version, 0);
        assert_eq!(domain.invocation_sequence, 0);
        assert!(!domain.resume_pending);
        assert_eq!(domain.payload, br#"{"titleId":"u1"}"#);
    }

    #[test]
    fn resume_delivery_copies_checkpoint_onto_domain_event() {
        let mut delivery = sample_delivery();
        delivery.checkpoint_json = Some(r#"{"offset":1}"#.into());
        delivery.checkpoint_schema_version = 2;
        delivery.invocation_sequence = 1;
        delivery.resume_pending = false;
        let domain = domain_event_for_delivery(&sample_event(), &delivery);
        assert_eq!(domain.checkpoint_json, r#"{"offset":1}"#);
        assert_eq!(domain.checkpoint_schema_version, 2);
        assert_eq!(domain.invocation_sequence, 1);
        assert!(domain.resume_pending);
    }
}
