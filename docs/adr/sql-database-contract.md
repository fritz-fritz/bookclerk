# ADR: SQL database plugins as thin adapters

- **Status:** Accepted
- **Date:** 2026-08-21
- **Related:** [#178](https://github.com/fritz-fritz/bookclerk/issues/178),
  [#177](https://github.com/fritz-fritz/bookclerk/pull/177),
  [Workers RPC + workerd](plugin-workers-rpc-workerd.md)

## Context

Bookclerk ships three first-party database guests (SQLite, PostgreSQL,
Cloudflare D1). Ordinary CRUD already crosses a generic SeaORM proxy
(`dbQuery` / `dbExecute`). Atomic work did not: the wire type was a named
domain enum (`deleteUser`, `publishDomainEvent`, `claimNextJob`, …). D1
recompiled each variant into schema-aware SQL inside the guest; SQLite and
PostgreSQL ran a separate SeaORM implementation of the same invariants.
Event claiming branched on `DatabaseBackend::Postgres` for
`pg_advisory_xact_lock`. Schema changes required coordinated edits in the
host and the D1 planner.

That made D1 a second Bookclerk repository, not a SQL adapter.

## Decision

### SQL-like databases only

Supported backends are SQLite, PostgreSQL, and Cloudflare D1 (SQLite
dialect over HTTP). Document, key-value, and other non-SQL stores are out
of scope. A guest that cannot meet the required semantics must **fail
capability negotiation** rather than silently weaken correctness.

### Ownership

| Layer | Owns |
| --- | --- |
| **Host / `bookclerk-library`** | Schema and migrations; domain operations; SQL and query plans; deduplication; leases and fences; queue/event state machines; result interpretation; idempotency policy |
| **Database plugin** | Connection and transport; negotiated SQL family and limits; bind encoding; generic query / execute / atomic-batch mechanics; error normalization; backend timing; unavoidable engine quirks |
| **ABI** | Generic, bounded execution primitives and capability negotiation. It must not require an adapter to understand users, jobs, books, events, or Bookclerk table names |

Domain names (`publishDomainEvent`, `claimNextJob`, `deleteUser`) stay in
host code. The ABI is an escape hatch for backend mechanics, not a second
repository interface.

### Capability negotiation

After `openSession` the host queries the `bookclerk.capabilities` sentinel.
The guest returns a JSON object (also flattened onto `DbConnectResult`):

- `sqlFamily`: `sqlite` \| `postgres`
- `interactiveTxn`, `atomicBatch`, `returning`
- `maxBinds`, `maxStatements`, `maxResultRows`, `maxPayloadBytes`
- `timing`

The host must not invent these from the plugin id. Missing required fields,
`atomicBatch: false`, or limits below the host's compiled minimums are a
hard error. Wake page size and `IN (…)` chunking are derived from
`maxBinds`.

First-party values: D1 `maxBinds = 100`; SQLite and PostgreSQL report the
engine bind cap (host still chunks conservatively).

### Generic atomic plan

Atomic work travels as `DatabaseSession.query("bookclerk.atomic", json)`
with a **plan body** (no Cap'n `DatabaseSession` method; guests already
special-case that SQL sentinel):

- caller `operationId` and canonical `requestHash`
- ordered parameterized statements (`query` \| `execute`)
- outcome selector (status column and/or affected-row predicate)
- optional payload selector
- uniform timing metadata on the result

The guest runs the statements as **one SQL transaction** (D1 HTTP
`{ "batch": [...] }`; SQLite/PostgreSQL `BEGIN`). Receipt rows live in
host-authored SQL against `db_atomic_receipts`. Guests must not parse
Bookclerk operation names.

Stable error categories: unique/constraint, retryable, unavailable,
timeout, unsupported.

Cancellation and deadlines stay RPC-level (existing session cancel).

### Portable concurrency

Domain/store code must not branch on a concrete database type for
correctness. Serialization uses schema-based slot rows
(`db_serialization_slots`): `INSERT` the key, then `UPDATE bump = bump + 1`
to take a write lock. PostgreSQL advisory locks and SQLite-only
`job_queue_control` dual-paths are not used for new atomic work.

JSON filter / catalog matching for event claim is evaluated in the host.
The atomic mutation is a compare-and-set on a concrete delivery id.

### Fail closed

A backend that cannot provide parameterized statements, reliable
affected-row counts, atomic batch or interactive transactions, durable
commit with replay-safe lost-response handling, or the negotiated bind
limits is not loaded.

## Consequences

- First-party database plugins shrink to connect, migrate (executing
  host-authored DDL), proxy CRUD, and a generic batch executor.
- An architecture lint forbids plugin sources from importing Bookclerk
  entities or embedding application table names.
- Equal performance across engines is not guaranteed.
- Integration plugins never receive database credentials or raw
  connections.
- Non-SQL backends remain unsupported by product policy, not by a
  missing adapter.
