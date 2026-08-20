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
    let domain = DomainEvent {
        event_id: event.id.clone(),
        event_type: event.event_type.clone(),
        schema_version: u32::try_from(event.schema_version).unwrap_or(1),
        occurred_at_unix_ms: u64::try_from(event.occurred_at.timestamp_millis()).unwrap_or(0),
        account_id: event.account_id.clone(),
        correlation_id: event.correlation_id.clone(),
        causation_id: event.causation_id.clone(),
        deduplication_key: delivery.idempotency_key.clone(),
        delivery_attempt: u32::try_from(delivery.attempt_count.max(1)).unwrap_or(1),
        payload: event.payload.into_bytes(),
    };
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
            info!(
                delivery_id = %delivery.id,
                plugin = %delivery.plugin_id,
                outcome = "retry",
                %reason,
                "event delivery retry scheduled"
            );
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
