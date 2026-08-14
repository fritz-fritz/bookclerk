//! Leased worker loop and startup reconciliation for the durable job queue.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use bookclerk_library::{JobRecord, JobResourceClass, JobState, LibraryError, LibraryStore};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};
use uuid::Uuid;

use crate::api::AppState;
use crate::job_handler::{InProcessJobTransport, JobCommand, JobExecCtx, JobTransport};
use crate::jobs::note_job_failure;

/// Reclaim leases, reconcile orphan acquire rows, sweep scratch dirs, start workers.
pub async fn start_job_runtime(state: Arc<AppState>) {
    if let Err(err) = reconcile_on_startup(&state).await {
        warn!(error = %err, "job startup reconciliation failed");
    }
    let n = state.config.read().await.jobs.concurrency.network.max(1);
    info!(network_workers = n, "starting durable job workers");
    for index in 0..n {
        spawn_network_worker(state.clone(), index);
    }
}

/// Reclaim expired leases, fail orphaned book rows, and delete unregistered scratch dirs.
pub async fn reconcile_on_startup(state: &AppState) -> anyhow::Result<()> {
    let library = state.library_snapshot().await;
    let reclaimed = library.reclaim_expired_leases().await?;
    let orphans = library.reconcile_orphaned_acquire_rows().await?;
    let cfg = state.config.read().await.clone();
    let pruned = library.prune_terminal_jobs(cfg.jobs.retention_days).await?;
    let cache = cfg.download_cache_dir();
    let swept = sweep_orphan_temp_dirs(&library, &cache).await?;
    info!(
        reclaimed,
        orphans, pruned, swept, "job queue reconciled at startup"
    );
    Ok(())
}

/// Delete `{cache}/acquire*` directories that are not registered to an active job.
pub async fn sweep_orphan_temp_dirs(
    library: &LibraryStore,
    cache_dir: &Path,
) -> anyhow::Result<u32> {
    let mut active_keep = HashSet::new();
    for row in library.list_all_job_temp_paths().await? {
        if let Ok(Some(job)) = library.get_job(&row.job_id).await {
            if job.state.is_active() {
                active_keep.insert(PathBuf::from(row.path));
            }
        }
    }
    let mut swept = 0u32;
    for name in ["acquire", "acquire-pdf"] {
        swept += sweep_dir(&cache_dir.join(name), &active_keep).await;
    }
    Ok(swept)
}

/// Deletes unregistered child directories under `root`.
async fn sweep_dir(root: &Path, keep: &HashSet<PathBuf>) -> u32 {
    let mut n = 0u32;
    let Ok(mut rd) = tokio::fs::read_dir(root).await else {
        return 0;
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        if keep.contains(&path) {
            continue;
        }
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        match tokio::fs::remove_dir_all(&path).await {
            Ok(()) => n += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                warn!(path = %path.display(), error = %err, "failed to sweep orphan work dir")
            }
        }
    }
    n
}

/// Spawns one `network` resource-class worker (claim / run / idle).
fn spawn_network_worker(state: Arc<AppState>, index: u32) {
    tokio::spawn(async move {
        let owner = format!("network-{index}-{}", Uuid::new_v4());
        let mut idle = tokio::time::interval(Duration::from_secs(5));
        idle.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            let permit = state.job_runtime.read().await;
            let library = state.library_snapshot().await;
            if let Err(err) = library.reclaim_expired_leases().await {
                warn!(error = %err, "lease reclaim failed");
            }
            let cfg = state.config.read().await.clone();
            let _ = library.prune_terminal_jobs(cfg.jobs.retention_days).await;
            match claim_with_replay(&library, &owner, cfg.jobs.lease_seconds).await {
                Ok(Some(job)) => {
                    run_claimed_job(state.clone(), &owner, job, cfg.jobs.lease_seconds).await;
                    drop(permit);
                }
                Ok(None) => {
                    drop(permit);
                    tokio::select! {
                        () = state.job_notify.notified() => {}
                        _ = idle.tick() => {}
                    }
                }
                Err(err) => {
                    drop(permit);
                    warn!(error = %err, "claim_next_job failed");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
}

/// Claims the next network job, retrying a lost RPC with the same operation id.
async fn claim_with_replay(
    library: &LibraryStore,
    owner: &str,
    lease_secs: u64,
) -> Result<Option<JobRecord>, LibraryError> {
    let operation_id = Uuid::new_v4().to_string();
    loop {
        match library
            .claim_next_job(JobResourceClass::Network, owner, lease_secs, &operation_id)
            .await
        {
            Ok(job) => return Ok(job),
            Err(LibraryError::Unavailable(_)) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(err) => return Err(err),
        }
    }
}

/// How the worker should treat one heartbeat RPC.
enum HeartbeatTick {
    /// Lease still owned; keep running.
    Renewed,
    /// Fence no longer matches; stop the handler and ignore its result.
    FenceLost,
    /// Database error; keep the lease assumption and retry later.
    Transient(LibraryError),
}

/// Classifies a heartbeat result without treating transport errors as fence loss.
fn classify_heartbeat(result: Result<bool, LibraryError>) -> HeartbeatTick {
    match result {
        Ok(true) => HeartbeatTick::Renewed,
        Ok(false) => HeartbeatTick::FenceLost,
        Err(err) => HeartbeatTick::Transient(err),
    }
}

/// How to finalize a handler that returned `Err`.
enum HandlerFailKind {
    /// Operator `cancel_requested` is set; mark the job cancelled.
    OperatorCancel,
    /// Local cancel without an operator flag (true fence loss); ignore the result.
    FenceLost,
    /// Ordinary handler failure; `fail_job` with retry/backoff.
    Handler,
}

/// Distinguishes operator cancel from local cancel caused by fence loss.
fn classify_handler_failure(local_cancel: bool, operator_cancel: bool) -> HandlerFailKind {
    if operator_cancel {
        HandlerFailKind::OperatorCancel
    } else if local_cancel {
        HandlerFailKind::FenceLost
    } else {
        HandlerFailKind::Handler
    }
}

/// Heartbeats the lease while the handler runs, then completes or fails the row.
async fn run_claimed_job(state: Arc<AppState>, _owner: &str, job: JobRecord, lease_secs: u64) {
    let Some(fence) = job.fence() else {
        warn!(job_id = %job.id, "claimed job missing lease owner; skipping");
        return;
    };
    let ctx = JobExecCtx {
        fence: fence.clone(),
        cancel: Arc::new(AtomicBool::new(false)),
    };
    if job.state == JobState::Cancelled || job.cancel_requested {
        ctx.request_cancel();
    }

    let heartbeat = {
        let state = state.clone();
        let fence = fence.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(lease_secs.max(10) / 3));
            tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let library = state.library_snapshot().await;
                match library.job_cancel_requested(&fence.job_id).await {
                    Ok(true) => ctx.request_cancel(),
                    Ok(false) => {}
                    Err(err) => {
                        warn!(
                            job_id = %fence.job_id,
                            error = %err,
                            "job_cancel_requested failed; continuing heartbeat"
                        );
                    }
                }
                match classify_heartbeat(library.heartbeat_job(&fence, lease_secs, None).await) {
                    HeartbeatTick::Renewed => {}
                    HeartbeatTick::FenceLost => {
                        ctx.request_cancel();
                        break;
                    }
                    HeartbeatTick::Transient(err) => {
                        warn!(
                            job_id = %fence.job_id,
                            error = %err,
                            "heartbeat_job failed; will retry next tick"
                        );
                    }
                }
            }
        })
    };

    let result = execute_claimed(state.clone(), &job, ctx.clone()).await;
    heartbeat.abort();
    let library = state.library_snapshot().await;
    match result {
        Ok(detail) => {
            info!(job_id = %fence.job_id, kind = job.kind.as_str(), %detail, "job succeeded");
            match library.complete_job(&fence, Some(&detail)).await {
                Ok(true) => {}
                Ok(false) => {
                    warn!(
                        job_id = %fence.job_id,
                        "fence lost; ignoring handler success"
                    );
                }
                Err(err) => {
                    warn!(job_id = %fence.job_id, error = %err, "failed to mark job succeeded")
                }
            }
        }
        Err(err) => {
            let operator_cancel = library
                .job_cancel_requested(&fence.job_id)
                .await
                .unwrap_or(false);
            match classify_handler_failure(ctx.is_cancelled(), operator_cancel) {
                HandlerFailKind::OperatorCancel => {
                    match library.fail_job(&fence, "cancelled", "cancelled").await {
                        Ok(true) => info!(job_id = %fence.job_id, "job cancelled"),
                        Ok(false) => {
                            warn!(
                                job_id = %fence.job_id,
                                "fence lost; ignoring cancel finalization"
                            );
                        }
                        Err(mark) => {
                            warn!(job_id = %fence.job_id, error = %mark, "failed to mark job cancelled")
                        }
                    }
                }
                HandlerFailKind::FenceLost => {
                    warn!(
                        job_id = %fence.job_id,
                        "fence lost; ignoring handler result"
                    );
                }
                HandlerFailKind::Handler => {
                    note_job_failure(&fence.job_id, &err);
                    match library.fail_job(&fence, "handler", &err.to_string()).await {
                        Ok(true) => {}
                        Ok(false) => {
                            warn!(
                                job_id = %fence.job_id,
                                "fence lost; ignoring handler failure"
                            );
                        }
                        Err(mark) => {
                            warn!(job_id = %fence.job_id, error = %mark, "failed to mark job failed")
                        }
                    }
                }
            }
        }
    }
}

/// Decodes the versioned command and runs it on the in-process transport.
async fn execute_claimed(
    state: Arc<AppState>,
    job: &JobRecord,
    ctx: JobExecCtx,
) -> anyhow::Result<String> {
    if ctx.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    let cmd = JobCommand::from_record(job)?;
    InProcessJobTransport::new(state).execute(cmd, ctx).await
}

/// Sleep `interval` with ±10% jitter (never negative).
pub fn jittered_delay(interval: Duration) -> Duration {
    let millis = interval.as_millis() as f64;
    let jitter = millis * 0.10;
    let offset = (rand::random::<f64>() * 2.0 - 1.0) * jitter;
    let adjusted = (millis + offset).max(0.0);
    Duration::from_millis(adjusted as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_stays_within_ten_percent() {
        let base = Duration::from_secs(100);
        for _ in 0..32 {
            let d = jittered_delay(base);
            assert!(d >= Duration::from_secs(90));
            assert!(d <= Duration::from_secs(110));
        }
    }

    #[tokio::test]
    async fn sweep_removes_unregistered_acquire_dirs() {
        use bookclerk_library::{EnqueueJobSpec, JobKind, JobPayload, JobTrigger};

        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let orphan = cache.join("acquire").join("orphan-title");
        let kept = cache.join("acquire").join("kept-title");
        tokio::fs::create_dir_all(&orphan).await.unwrap();
        tokio::fs::create_dir_all(&kept).await.unwrap();
        tokio::fs::write(orphan.join("x"), b"x").await.unwrap();
        tokio::fs::write(kept.join("y"), b"y").await.unwrap();

        let store = LibraryStore::from_connection(
            bookclerk_plugin_database_sqlite::open_memory()
                .await
                .unwrap(),
        );
        let created = store
            .enqueue_job(EnqueueJobSpec {
                kind: JobKind::Acquire,
                payload: JobPayload {
                    account: None,
                    title: Some("kept".into()),
                    trigger: JobTrigger::Api,
                    ..Default::default()
                },
                priority: 0,
                max_attempts: 3,
                max_pending: 8,
                run_after: None,
            })
            .await
            .unwrap();
        let bookclerk_library::EnqueueOutcome::Created { id } = created else {
            panic!("expected created");
        };
        store
            .register_job_temp_path(&id, &kept.to_string_lossy())
            .await
            .unwrap();

        let swept = sweep_orphan_temp_dirs(&store, &cache).await.unwrap();
        assert_eq!(swept, 1);
        assert!(!orphan.exists());
        assert!(kept.exists());
    }

    #[test]
    fn heartbeat_ok_true_renews_lease() {
        assert!(matches!(
            classify_heartbeat(Ok(true)),
            HeartbeatTick::Renewed
        ));
    }

    #[test]
    fn heartbeat_ok_false_is_fence_loss() {
        assert!(matches!(
            classify_heartbeat(Ok(false)),
            HeartbeatTick::FenceLost
        ));
    }

    #[test]
    fn heartbeat_db_error_is_not_fence_loss() {
        let tick = classify_heartbeat(Err(LibraryError::NotFound("jobs".into())));
        assert!(matches!(tick, HeartbeatTick::Transient(_)));
    }

    #[test]
    fn handler_err_after_local_cancel_is_not_marked_cancelled() {
        assert!(matches!(
            classify_handler_failure(true, false),
            HandlerFailKind::FenceLost
        ));
    }

    #[test]
    fn operator_cancel_wins_over_local_cancel_flag() {
        assert!(matches!(
            classify_handler_failure(true, true),
            HandlerFailKind::OperatorCancel
        ));
    }

    #[test]
    fn handler_err_without_cancel_is_ordinary_failure() {
        assert!(matches!(
            classify_handler_failure(false, false),
            HandlerFailKind::Handler
        ));
    }
}
