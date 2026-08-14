//! Durable job-queue methods on [`LibraryStore`].

use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait,
    ActiveValue::{NotSet, Set},
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};
use uuid::Uuid;

use super::{now_str, parse_dt, parse_dt_opt, LibraryStore};
use crate::entities::{books, job_temp_paths, jobs};
use crate::error::{LibraryError, Result};
use crate::models::{
    job_backoff_run_after, AcquireStatus, BookRecord, EnqueueJobSpec, EnqueueOutcome, JobKind,
    JobPayload, JobRecord, JobResourceClass, JobState, JobTempPath,
};

impl LibraryStore {
    /// Admit a job, coalescing onto an active row with the same dedup key.
    ///
    /// # Arguments
    ///
    /// * `spec` - Kind, payload, priority, attempt cap, and pending cap.
    ///
    /// # Returns
    ///
    /// [`EnqueueOutcome::Created`], [`EnqueueOutcome::Duplicate`], or
    /// [`EnqueueOutcome::QueueFull`].
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the database write fails.
    pub async fn enqueue_job(&self, spec: EnqueueJobSpec) -> Result<EnqueueOutcome> {
        let dedup_key = spec.kind.dedup_key(&spec.payload);
        if let Some(existing) = self.find_active_job_by_dedup(&dedup_key).await? {
            return Ok(EnqueueOutcome::Duplicate {
                existing_id: existing.id,
            });
        }
        let active = self.count_active_jobs().await?;
        if active >= spec.max_pending.max(0) {
            return Ok(EnqueueOutcome::QueueFull);
        }
        let now = Utc::now();
        let run_after = spec.run_after.unwrap_or(now);
        let id = format!("{}-{}", spec.kind.as_str(), Uuid::new_v4());
        let payload = serde_json::to_string(&spec.payload).unwrap_or_else(|_| "{}".into());
        let now_s = now.to_rfc3339();
        let am = jobs::ActiveModel {
            id: Set(id.clone()),
            kind: Set(spec.kind.as_str().to_string()),
            state: Set(JobState::Pending.as_str().to_string()),
            priority: Set(spec.priority),
            resource_class: Set(spec.kind.resource_class().as_str().to_string()),
            payload: Set(payload),
            progress: Set(None),
            attempt_count: Set(0),
            max_attempts: Set(spec.max_attempts.max(1)),
            run_after: Set(run_after.to_rfc3339()),
            lease_owner: Set(None),
            lease_expires_at: Set(None),
            dedup_key: Set(dedup_key.clone()),
            error_kind: Set(None),
            error_message: Set(None),
            cancel_requested: Set(0),
            created_at: Set(now_s.clone()),
            updated_at: Set(now_s),
            started_at: Set(None),
            finished_at: Set(None),
        };
        am.insert(&self.db).await.map_err(LibraryError::Orm)?;
        // Two concurrent admits can both pass the pre-check; keep the oldest.
        if let Some(winner) = self.find_active_job_by_dedup(&dedup_key).await? {
            if winner.id != id {
                let _ = jobs::Entity::delete_by_id(&id).exec(&self.db).await;
                return Ok(EnqueueOutcome::Duplicate {
                    existing_id: winner.id,
                });
            }
        }
        Ok(EnqueueOutcome::Created { id })
    }

    /// Claim the next ready job in `resource_class` for `owner`.
    ///
    /// Increments `attempt_count` and sets a lease of `lease_secs` seconds.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the claim write fails.
    pub async fn claim_next_job(
        &self,
        resource_class: JobResourceClass,
        owner: &str,
        lease_secs: u64,
    ) -> Result<Option<JobRecord>> {
        let now = Utc::now();
        let now_s = now.to_rfc3339();
        let candidates = jobs::Entity::find()
            .filter(jobs::Column::ResourceClass.eq(resource_class.as_str()))
            .filter(jobs::Column::State.eq(JobState::Pending.as_str()))
            .filter(jobs::Column::RunAfter.lte(now_s.clone()))
            .order_by_desc(jobs::Column::Priority)
            .order_by_asc(jobs::Column::CreatedAt)
            .limit(8)
            .all(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        let lease_expires =
            (now + Duration::seconds(i64::try_from(lease_secs).unwrap_or(60))).to_rfc3339();
        for model in candidates {
            if model.cancel_requested != 0 {
                self.mark_job_cancelled(&model.id, "cancelled", "cancelled by operator")
                    .await?;
                continue;
            }
            let attempt = model.attempt_count + 1;
            let started = model.started_at.clone().unwrap_or_else(|| now_s.clone());
            let mut am: jobs::ActiveModel = model.into();
            am.state = Set(JobState::Running.as_str().to_string());
            am.attempt_count = Set(attempt);
            am.lease_owner = Set(Some(owner.to_string()));
            am.lease_expires_at = Set(Some(lease_expires.clone()));
            am.started_at = Set(Some(started));
            am.updated_at = Set(now_s.clone());
            am.error_kind = Set(None);
            am.error_message = Set(None);
            let updated = am.update(&self.db).await.map_err(LibraryError::Orm)?;
            if updated.lease_owner.as_deref() == Some(owner)
                && updated.state == JobState::Running.as_str()
            {
                return Ok(Some(map_job(updated)));
            }
        }
        Ok(None)
    }

    /// Refresh the lease and optional progress for a running job owned by `owner`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn heartbeat_job(
        &self,
        id: &str,
        owner: &str,
        lease_secs: u64,
        progress: Option<&str>,
    ) -> Result<bool> {
        let Some(model) = jobs::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(false);
        };
        if model.state != JobState::Running.as_str() || model.lease_owner.as_deref() != Some(owner)
        {
            return Ok(false);
        }
        let now = Utc::now();
        let mut am: jobs::ActiveModel = model.into();
        am.lease_expires_at = Set(Some(
            (now + Duration::seconds(i64::try_from(lease_secs).unwrap_or(60))).to_rfc3339(),
        ));
        am.updated_at = Set(now.to_rfc3339());
        if let Some(progress) = progress {
            am.progress = Set(Some(progress.to_string()));
        }
        am.update(&self.db).await.map_err(LibraryError::Orm)?;
        Ok(true)
    }

    /// Persist a human-readable progress string without touching the lease.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn set_job_progress(&self, id: &str, progress: &str) -> Result<()> {
        let Some(model) = jobs::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(());
        };
        let mut am: jobs::ActiveModel = model.into();
        am.progress = Set(Some(progress.to_string()));
        am.updated_at = Set(now_str());
        am.update(&self.db).await.map_err(LibraryError::Orm)?;
        Ok(())
    }

    /// Mark a running or pending job succeeded.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn complete_job(&self, id: &str, progress: Option<&str>) -> Result<()> {
        let Some(model) = jobs::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(());
        };
        let now = now_str();
        let mut am: jobs::ActiveModel = model.into();
        am.state = Set(JobState::Succeeded.as_str().to_string());
        if let Some(progress) = progress {
            am.progress = Set(Some(progress.to_string()));
        }
        am.lease_owner = Set(None);
        am.lease_expires_at = Set(None);
        am.finished_at = Set(Some(now.clone()));
        am.updated_at = Set(now);
        am.update(&self.db).await.map_err(LibraryError::Orm)?;
        Ok(())
    }

    /// Fail a job, retrying with backoff when attempts remain.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn fail_job(&self, id: &str, error_kind: &str, error_message: &str) -> Result<()> {
        let Some(model) = jobs::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(());
        };
        if model.cancel_requested != 0 {
            return self
                .mark_job_cancelled(id, "cancelled", "cancelled by operator")
                .await;
        }
        let now = Utc::now();
        let retry = model.attempt_count < model.max_attempts;
        let attempt_count = model.attempt_count;
        let mut am: jobs::ActiveModel = model.into();
        am.error_kind = Set(Some(error_kind.to_string()));
        am.error_message = Set(Some(error_message.to_string()));
        am.lease_owner = Set(None);
        am.lease_expires_at = Set(None);
        am.updated_at = Set(now.to_rfc3339());
        if retry {
            am.state = Set(JobState::Pending.as_str().to_string());
            am.run_after = Set(job_backoff_run_after(attempt_count, now).to_rfc3339());
            am.finished_at = Set(None);
        } else {
            am.state = Set(JobState::Failed.as_str().to_string());
            am.finished_at = Set(Some(now.to_rfc3339()));
        }
        am.update(&self.db).await.map_err(LibraryError::Orm)?;
        Ok(())
    }

    /// Cancel a pending job immediately, or flag a running job for cooperative stop.
    ///
    /// # Returns
    ///
    /// The updated record, or `None` when the id is unknown or already terminal.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn request_job_cancel(&self, id: &str) -> Result<Option<JobRecord>> {
        let Some(model) = jobs::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(None);
        };
        let state = JobState::parse(&model.state).unwrap_or(JobState::Failed);
        if state.is_terminal() {
            return Ok(Some(map_job(model)));
        }
        if state == JobState::Pending {
            self.mark_job_cancelled(id, "cancelled", "cancelled by operator")
                .await?;
            return self.get_job(id).await;
        }
        let mut am: jobs::ActiveModel = model.into();
        am.cancel_requested = Set(1);
        am.updated_at = Set(now_str());
        let updated = am.update(&self.db).await.map_err(LibraryError::Orm)?;
        Ok(Some(map_job(updated)))
    }

    /// True when a running worker should abort after the current step.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn job_cancel_requested(&self, id: &str) -> Result<bool> {
        let Some(model) = jobs::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(false);
        };
        Ok(model.cancel_requested != 0)
    }

    /// Reclaim expired running leases: retry with backoff or fail at max attempts.
    ///
    /// # Returns
    ///
    /// Number of rows moved out of `running`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the update fails.
    pub async fn reclaim_expired_leases(&self) -> Result<u32> {
        let now = Utc::now();
        let now_s = now.to_rfc3339();
        let running = jobs::Entity::find()
            .filter(jobs::Column::State.eq(JobState::Running.as_str()))
            .all(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        let mut n = 0u32;
        for model in running {
            let expired = model
                .lease_expires_at
                .as_deref()
                .map(parse_dt)
                .is_none_or(|exp| exp <= now);
            if !expired {
                continue;
            }
            if model.cancel_requested != 0 {
                self.mark_job_cancelled(&model.id, "cancelled", "cancelled by operator")
                    .await?;
                n += 1;
                continue;
            }
            let terminal = model.attempt_count >= model.max_attempts;
            let mut am: jobs::ActiveModel = model.into();
            am.lease_owner = Set(None);
            am.lease_expires_at = Set(None);
            am.updated_at = Set(now_s.clone());
            am.error_kind = Set(Some("lease_expired".into()));
            am.error_message = Set(Some(
                "worker lease expired; reclaiming after restart".into(),
            ));
            if terminal {
                am.state = Set(JobState::Failed.as_str().to_string());
                am.finished_at = Set(Some(now_s.clone()));
            } else {
                am.state = Set(JobState::Pending.as_str().to_string());
                am.run_after = Set(now_s.clone());
                am.finished_at = Set(None);
            }
            am.update(&self.db).await.map_err(LibraryError::Orm)?;
            n += 1;
        }
        Ok(n)
    }

    /// Load one job by id.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn get_job(&self, id: &str) -> Result<Option<JobRecord>> {
        Ok(jobs::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
            .map(map_job))
    }

    /// List recent jobs, newest first, capped at `limit` (minimum 1).
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn list_jobs(&self, limit: u64) -> Result<Vec<JobRecord>> {
        Ok(jobs::Entity::find()
            .order_by_desc(jobs::Column::CreatedAt)
            .limit(limit.max(1))
            .all(&self.db)
            .await
            .map_err(LibraryError::Orm)?
            .into_iter()
            .map(map_job)
            .collect())
    }

    /// Delete terminal jobs older than `retention_days`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the delete fails.
    pub async fn prune_terminal_jobs(&self, retention_days: u64) -> Result<u64> {
        let cutoff =
            (Utc::now() - Duration::days(i64::try_from(retention_days).unwrap_or(7))).to_rfc3339();
        let res = jobs::Entity::delete_many()
            .filter(jobs::Column::State.is_in([
                JobState::Succeeded.as_str(),
                JobState::Failed.as_str(),
                JobState::Cancelled.as_str(),
            ]))
            .filter(jobs::Column::FinishedAt.lte(cutoff))
            .exec(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(res.rows_affected)
    }

    /// Register a scratch path so crash recovery can clean it up.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the insert fails.
    pub async fn register_job_temp_path(&self, job_id: &str, path: &str) -> Result<()> {
        let existing = job_temp_paths::Entity::find()
            .filter(job_temp_paths::Column::JobId.eq(job_id))
            .filter(job_temp_paths::Column::Path.eq(path))
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        if existing.is_some() {
            return Ok(());
        }
        let am = job_temp_paths::ActiveModel {
            id: NotSet,
            job_id: Set(job_id.to_string()),
            path: Set(path.to_string()),
            created_at: Set(now_str()),
        };
        am.insert(&self.db).await.map_err(LibraryError::Orm)?;
        Ok(())
    }

    /// List scratch paths registered for `job_id`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn list_job_temp_paths(&self, job_id: &str) -> Result<Vec<JobTempPath>> {
        Ok(job_temp_paths::Entity::find()
            .filter(job_temp_paths::Column::JobId.eq(job_id))
            .all(&self.db)
            .await
            .map_err(LibraryError::Orm)?
            .into_iter()
            .map(map_temp_path)
            .collect())
    }

    /// List every registered scratch path (used by the startup sweeper).
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn list_all_job_temp_paths(&self) -> Result<Vec<JobTempPath>> {
        Ok(job_temp_paths::Entity::find()
            .all(&self.db)
            .await
            .map_err(LibraryError::Orm)?
            .into_iter()
            .map(map_temp_path)
            .collect())
    }

    /// Drop scratch-path rows for `job_id` after cleanup.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the delete fails.
    pub async fn clear_job_temp_paths(&self, job_id: &str) -> Result<()> {
        job_temp_paths::Entity::delete_many()
            .filter(job_temp_paths::Column::JobId.eq(job_id))
            .exec(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(())
    }

    /// True when any acquire job is currently running.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn has_running_acquire_job(&self) -> Result<bool> {
        let n = jobs::Entity::find()
            .filter(jobs::Column::Kind.eq(JobKind::Acquire.as_str()))
            .filter(jobs::Column::State.eq(JobState::Running.as_str()))
            .count(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(n > 0)
    }

    /// Books left in `queued` / `downloading` with no running acquire job.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn list_orphaned_acquire_books(&self) -> Result<Vec<BookRecord>> {
        if self.has_running_acquire_job().await? {
            return Ok(Vec::new());
        }
        let rows = books::Entity::find()
            .filter(books::Column::AcquireStatus.is_in([
                AcquireStatus::Queued.as_str(),
                AcquireStatus::Downloading.as_str(),
            ]))
            .all(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(super::map_book(row)?);
        }
        Ok(out)
    }

    /// Mark orphaned `queued` / `downloading` books as `error` after a crash.
    ///
    /// # Returns
    ///
    /// Number of book rows updated.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when a status write fails.
    pub async fn reconcile_orphaned_acquire_rows(&self) -> Result<u32> {
        let orphans = self.list_orphaned_acquire_books().await?;
        let mut n = 0u32;
        for book in orphans {
            self.set_acquire_status(
                &book.uuid,
                &book.account_id,
                AcquireStatus::Error,
                book.storage_key.as_deref(),
                Some("orphaned_after_restart"),
            )
            .await?;
            n += 1;
        }
        Ok(n)
    }

    /// Count pending + running jobs (admission cap).
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Orm`] when the read fails.
    pub async fn count_active_jobs(&self) -> Result<i64> {
        let n = jobs::Entity::find()
            .filter(
                jobs::Column::State.is_in([JobState::Pending.as_str(), JobState::Running.as_str()]),
            )
            .count(&self.db)
            .await
            .map_err(LibraryError::Orm)?;
        Ok(i64::try_from(n).unwrap_or(i64::MAX))
    }

    /// Loads the oldest pending/running row with `dedup_key`, if any.
    async fn find_active_job_by_dedup(&self, dedup_key: &str) -> Result<Option<JobRecord>> {
        Ok(jobs::Entity::find()
            .filter(jobs::Column::DedupKey.eq(dedup_key))
            .filter(
                jobs::Column::State.is_in([JobState::Pending.as_str(), JobState::Running.as_str()]),
            )
            .order_by_asc(jobs::Column::CreatedAt)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
            .map(map_job))
    }

    /// Mark a pending or running job cancelled with a structured error.
    async fn mark_job_cancelled(
        &self,
        id: &str,
        error_kind: &str,
        error_message: &str,
    ) -> Result<()> {
        let Some(model) = jobs::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(LibraryError::Orm)?
        else {
            return Ok(());
        };
        let now = now_str();
        let mut am: jobs::ActiveModel = model.into();
        am.state = Set(JobState::Cancelled.as_str().to_string());
        am.cancel_requested = Set(1);
        am.error_kind = Set(Some(error_kind.to_string()));
        am.error_message = Set(Some(error_message.to_string()));
        am.lease_owner = Set(None);
        am.lease_expires_at = Set(None);
        am.finished_at = Set(Some(now.clone()));
        am.updated_at = Set(now);
        am.update(&self.db).await.map_err(LibraryError::Orm)?;
        Ok(())
    }
}

/// Maps a `jobs` row to [`JobRecord`], parsing enums and RFC 3339 timestamps.
fn map_job(m: jobs::Model) -> JobRecord {
    let payload = serde_json::from_str::<JobPayload>(&m.payload).unwrap_or_default();
    JobRecord {
        id: m.id,
        kind: JobKind::parse(&m.kind).unwrap_or(JobKind::Scan),
        state: JobState::parse(&m.state).unwrap_or(JobState::Failed),
        priority: m.priority,
        resource_class: JobResourceClass::parse(&m.resource_class)
            .unwrap_or(JobResourceClass::Network),
        payload,
        progress: m.progress,
        attempt_count: m.attempt_count,
        max_attempts: m.max_attempts,
        run_after: parse_dt(&m.run_after),
        lease_owner: m.lease_owner,
        lease_expires_at: parse_dt_opt(m.lease_expires_at.as_deref()),
        dedup_key: m.dedup_key,
        error_kind: m.error_kind,
        error_message: m.error_message,
        cancel_requested: m.cancel_requested != 0,
        created_at: parse_dt(&m.created_at),
        updated_at: parse_dt(&m.updated_at),
        started_at: parse_dt_opt(m.started_at.as_deref()),
        finished_at: parse_dt_opt(m.finished_at.as_deref()),
    }
}

/// Maps a `job_temp_paths` row to [`JobTempPath`].
fn map_temp_path(m: job_temp_paths::Model) -> JobTempPath {
    JobTempPath {
        id: m.id,
        job_id: m.job_id,
        path: m.path,
        created_at: parse_dt(&m.created_at),
    }
}
