# Durable job queue

`bookclerkd` admits background work as **durable rows** in `library.db` (`jobs`
+ `job_temp_paths`). HTTP and the interval scheduler are producers; a leased
worker loop claims jobs. There is no external broker.

This is not a general pub/sub bus. Integration `book_acquired` / plugin
`onEvent` stay separate. Future transcription / EPUB work (#121) should add a
new `kind` and `resource_class` on this table.

See [architecture.md](architecture.md) and [operations.md](operations.md).

## State machine

```text
pending → running → succeeded
                 ↘ failed (retry → pending with backoff, or terminal)
                 ↘ cancelled
```

- **Admission** writes `pending` (or returns an existing active row).
- A **worker** claims with a lease (`running`) and heartbeats.
- Expired leases are reclaimed to `pending` (or `failed` at `max_attempts`).
- `POST /api/jobs/{id}/cancel` cancels `pending` immediately and flags
  `running` for a cooperative stop between acquire titles.

## Job kinds

| Kind | Dedup key | Resource class | Handler |
| --- | --- | --- | --- |
| `scan` | `scan:account={id\|all}` | `network` | `run_scan` |
| `acquire` | `acquire:title={id\|all}:account={id\|all}` | `network` | `run_acquire` |
| `listen_sync` | `listen_sync` | `network` | `run_listen_sync` |

Reserved classes (no worker in this release): `media`, `transcription`,
`indexing`.

## Admission and backpressure

`POST /api/library/scan` and `/acquire` (and the scheduler) call
`LibraryStore::enqueue_job`:

| Outcome | HTTP | Meaning |
| --- | --- | --- |
| Created | 200 | New row; worker is notified |
| Duplicate | 409 | Same dedup key already `pending` or `running`; `job_id` is the existing row |
| QueueFull | 429 | `pending + running >= jobs.max_pending` |

CLI `bookclerk library scan/acquire` still runs in-process and does not use
this queue.

## Leases and crash recovery

- Default lease is 60s (`jobs.lease_seconds`); the worker heartbeats at
  `lease/3`.
- Startup and each worker tick call `reclaim_expired_leases`.
- Books left `queued` / `downloading` with **no** running acquire job are set
  to `error` (`orphaned_after_restart`). The next acquire job retries them.
- Scratch dirs under `{cache}/acquire` and `{cache}/acquire-pdf` are registered
  on the job and removed on success, failure, cancel, and startup sweep of
  unregistered orphans. New scratch is refused when usage exceeds
  `jobs.temp_quota_bytes` (default 2 GiB).

Scan and listen-sync retries are idempotent (upserts). Acquire retries skip
titles already `acquired`.

## Resource-class concurrency

`[jobs.concurrency].network` defaults to **1** (same single-writer profile as
the historical `work_lock`, which still wraps `run_*`). Codec work stays in
the `[media].workers` pool.

## Adding a job kind

1. Add a `JobKind` variant and dedup-key rule in
   [`crates/bookclerk-library/src/models.rs`](../crates/bookclerk-library/src/models.rs).
2. Implement a handler and dispatch it from
   [`crates/bookclerkd/src/job_worker.rs`](../crates/bookclerkd/src/job_worker.rs).
3. Enqueue from the API and/or scheduler with the same `EnqueueJobSpec`.
4. Register scratch paths via `LibraryStore::register_job_temp_path` when the
   job creates temp files.
5. Add dedup and restart tests.

## Configuration

See [configuration.md](configuration.md) (`[jobs]` and `BOOKCLERK_JOBS_*`).
