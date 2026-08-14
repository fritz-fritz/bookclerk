# Durable job queue

`bookclerkd` admits background work as **durable rows** in `library.db` (`jobs`
+ `job_temp_paths`). HTTP and the interval scheduler are producers; leased
workers claim jobs. There is no external broker.

This is not a general pub/sub bus. Domain events such as `book.acquired` /
plugin `onEvent` stay on a separate notification path (today:
`notify_integrations`). They must not become job kinds. A transactional outbox
for those events is a later change.

See [architecture.md](architecture.md) and [operations.md](operations.md).

## Command envelope

Each row stores a versioned JSON command (`JobPayload.v`, currently `1`).
Unknown kinds, unsupported versions, and malformed JSON are rewritten to
`kind=invalid` / `state=failed` / `error_kind=invalid_job` and never run a
handler.

Dispatch goes through `JobTransport` (`InProcessJobTransport` today; a workerd
adapter can replace it later). Handlers receive a `JobExecCtx` with the lease
fence and a cooperative cancel flag.

## State machine

```text
pending → running → succeeded
                 ↘ failed (retry → pending with backoff, or terminal)
                 ↘ cancelled
```

- **Admission** is one atomic backend operation (`dbAtomic` in production,
  a local `BEGIN` in tests). A partial unique index on `dedup_key` for
  `pending`/`running` rows enforces active-key uniqueness.
- A **worker** claims with a conditional `pending` → `running` update and a
  unique per-attempt `lease_generation`. Lost RPCs retry the same
  `operation_id` so a replay cannot claim a second row.
- Heartbeat, progress, complete, fail, and reclaim are fenced on
  `(job_id, lease_owner, lease_generation)`. Zero rows means the fence is
  lost; the worker cancels the handler and ignores the unfenced result.
- Reclaim also requires `lease_expires_at` to still be null or `<= now`, so a
  heartbeat that extends the same generation cannot be stolen.
- `POST /api/jobs/{id}/cancel` cancels `pending` immediately and flags
  `running` for a cooperative stop between acquire titles, scan sources, and
  listen-sync providers. Integration `scan_library` RPCs are not interruptible;
  cancel is checked before the call and a repeat remote scan is idempotent.

## Job kinds

| Kind | Dedup key | Resource class | Handler |
| --- | --- | --- | --- |
| `scan` | `scan:account={id\|all}` | `network` | `run_scan` |
| `acquire` | `acquire:title={id\|all}:account={id\|all}` | `network` | `run_acquire` |
| `listen_sync` | `listen_sync` | `network` | `run_listen_sync` |
| `integration_scan` | `integration_scan:id={id}` | `network` | `run_integration_scan` |

Reserved classes (no worker in this release): `media`, `transcription`,
`indexing`.

## Admission and backpressure

`POST /api/library/scan`, `/acquire`, `/api/discover/sync-listening`, and
`/integrations/{id}/scan` (and the scheduler) call
`LibraryStore::enqueue_job`:

| Outcome | HTTP | Meaning |
| --- | --- | --- |
| Created | 200 | New row; worker is notified |
| Duplicate | 409 | Same dedup key already `pending` or `running`; `job_id` is the existing row |
| QueueFull | 429 | `pending + running >= jobs.max_pending` |

Admission, claim, and handler execution share an `RwLock` (`job_runtime`).
A database plugin swap takes the write permit so it cannot observe an idle
worker that is about to claim. Admission waits up to 15s for a read permit
and returns `503` if a swap holds the lock too long.

PostgreSQL admission and scratch-quota updates take
`pg_advisory_xact_lock(88118)` (SQLite/D1 lock the `job_queue_control`
singleton) so `COUNT` then `INSERT` cannot exceed `max_pending` under
`READ COMMITTED`.

D1 claim marks pending rows with malformed JSON, an unknown kind, or an
unsupported envelope version as `invalid_job` in the same batch before
`json(payload)` runs, so a bad highest-priority row cannot abort and
poison the queue.

CLI `bookclerk library scan/acquire` still runs in-process and does not use
this queue.

## Leases and crash recovery

- Default lease is 60s (`jobs.lease_seconds`); the worker heartbeats at
  `lease/3`. Losing a heartbeat sets the handler cancel flag.
- Startup and each worker tick call `reclaim_expired_leases`.
- Books left `queued` / `downloading` with **no** running acquire job are set
  to `error` (`orphaned_after_restart`). The next acquire job retries them.
- Scratch dirs under `{cache}/acquire` and `{cache}/acquire-pdf` are reserved
  against `jobs.temp_quota_bytes` (default 2 GiB) and unregistered only after
  that path is deleted (or already gone). Startup sweeps unregistered orphans.
  Fetch is bounded by the remaining quota: sources can call
  `FetchOptions::enforce_cache_budget`, and the pipeline watchdog cancels a
  fetch that crosses the budget mid-write.

Scan, listen-sync, and integration-scan retries are idempotent (upserts /
remote rescan). Acquire retries skip titles already `acquired`.

## Resource-class concurrency

`[jobs.concurrency].network` is the number of leased network workers (default
**1**). The historical `work_lock` still wraps `run_*` so store rate limits
and the shared `StorageIndex` stay single-writer. Codec work stays in the
`[media].workers` pool.

## Adding a job kind

1. Add a `JobKind` variant, envelope fields, and dedup-key rule in
   [`crates/bookclerk-library/src/models.rs`](../crates/bookclerk-library/src/models.rs).
2. Add a `JobCommand` arm on [`InProcessJobTransport`](../crates/bookclerkd/src/job_handler.rs).
3. Enqueue from the API and/or scheduler with the same `EnqueueJobSpec`.
4. Reserve scratch paths via `LibraryStore::reserve_job_temp_path` when the
   job creates temp files; unregister one path after it is gone.
5. Add concurrent admission/claim, fence-loss, and restart tests.

## Configuration

See [configuration.md](configuration.md) (`[jobs]` and `BOOKCLERK_JOBS_*`).
