//! Leased worker loop and startup reconciliation for the durable job queue.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bookclerk_library::{JobKind, JobRecord, JobResourceClass, JobState};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};
use uuid::Uuid;

use crate::api::AppState;
use crate::jobs::{note_job_failure, run_acquire, run_listen_sync, run_scan};

/// Reclaim leases, reconcile orphan acquire rows, sweep scratch dirs, start workers.
pub async fn start_job_runtime(state: Arc<AppState>) {
    if let Err(err) = reconcile_on_startup(&state).await {
        warn!(error = %err, "job startup reconciliation failed");
    }
    spawn_network_worker(state);
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
    library: &bookclerk_library::LibraryStore,
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

/// Spawns the single `network` resource-class worker (claim / run / idle).
fn spawn_network_worker(state: Arc<AppState>) {
    tokio::spawn(async move {
        let owner = format!("network-{}", Uuid::new_v4());
        let mut idle = tokio::time::interval(Duration::from_secs(5));
        idle.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            let library = state.library_snapshot().await;
            if let Err(err) = library.reclaim_expired_leases().await {
                warn!(error = %err, "lease reclaim failed");
            }
            let cfg = state.config.read().await.clone();
            let _ = library.prune_terminal_jobs(cfg.jobs.retention_days).await;
            match library
                .claim_next_job(JobResourceClass::Network, &owner, cfg.jobs.lease_seconds)
                .await
            {
                Ok(Some(job)) => {
                    run_claimed_job(state.clone(), &owner, job, cfg.jobs.lease_seconds).await;
                }
                Ok(None) => {
                    tokio::select! {
                        () = state.job_notify.notified() => {}
                        _ = idle.tick() => {}
                    }
                }
                Err(err) => {
                    warn!(error = %err, "claim_next_job failed");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
}

/// Heartbeats the lease while the handler runs, then completes or fails the row.
async fn run_claimed_job(state: Arc<AppState>, owner: &str, job: JobRecord, lease_secs: u64) {
    let job_id = job.id.clone();
    let heartbeat = {
        let state = state.clone();
        let job_id = job_id.clone();
        let owner = owner.to_string();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(lease_secs.max(10) / 3));
            tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let library = state.library_snapshot().await;
                if !library
                    .heartbeat_job(&job_id, &owner, lease_secs, None)
                    .await
                    .unwrap_or(false)
                {
                    break;
                }
            }
        })
    };

    let result = execute_job(&state, &job).await;
    heartbeat.abort();
    let library = state.library_snapshot().await;
    match result {
        Ok(detail) => {
            info!(%job_id, kind = job.kind.as_str(), %detail, "job succeeded");
            if let Err(err) = library.complete_job(&job_id, Some(&detail)).await {
                warn!(%job_id, error = %err, "failed to mark job succeeded");
            }
        }
        Err(err) => {
            if library.job_cancel_requested(&job_id).await.unwrap_or(false) {
                let _ = library.request_job_cancel(&job_id).await;
                info!(%job_id, "job cancelled");
            } else {
                note_job_failure(&job_id, &err);
                if let Err(mark) = library.fail_job(&job_id, "handler", &err.to_string()).await {
                    warn!(%job_id, error = %mark, "failed to mark job failed");
                }
            }
        }
    }
}

/// Dispatches a claimed row to `run_scan` / `run_acquire` / `run_listen_sync`.
async fn execute_job(state: &AppState, job: &JobRecord) -> anyhow::Result<String> {
    if job.state == JobState::Cancelled || job.cancel_requested {
        anyhow::bail!("cancelled");
    }
    match job.kind {
        JobKind::Scan => run_scan(state, job.payload.account.as_deref()).await,
        JobKind::Acquire => {
            run_acquire(
                state,
                job.payload.title.as_deref(),
                job.payload.account.as_deref(),
                Some(job.id.as_str()),
            )
            .await
        }
        JobKind::ListenSync => run_listen_sync(state).await,
    }
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
        use bookclerk_library::{EnqueueJobSpec, JobKind, JobPayload, JobTrigger, LibraryStore};

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
}
