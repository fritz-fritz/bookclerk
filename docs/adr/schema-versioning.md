# ADR: Frozen host schema versions, checksums, and CLI downgrade

- **Status:** Accepted
- **Date:** 2026-08-26
- **Related:** [SQL database contract](sql-database-contract.md),
  [database.md](../database.md), [migration.md](../migration.md)

## Context

Bookclerk’s library schema used to grow as an incremental chain (named
fragments V2–V29, bookkeeping version 28 for the D1 event-table rebuild).
No production databases exist, so that chain is not an upgrade path. The
host still auto-applies remaining DDL on library open. An older binary
must not silently skip a newer database, and operators need an explicit
way to walk back to a schema this binary understands.

Plugin binding databases (`plugin_databases` plus per-binding units) are
plugin-owned DDL. The host must not run a second migrator inside them.

## Decision

### Schema version vs app version

- `schema_migrations.version` / SQLite `PRAGMA user_version` is a
  **monotonic integer** (`SCHEMA_VERSION`). It is not semver.
- `schema_migrations.app_version` records `CARGO_PKG_VERSION` at apply
  time for support notes only.
- `HostMigrationStep.introduced_in` is the workspace semver that first
  **froze** that integer (schema 1 is `"0.1.0"`).

### Freeze and `UNRELEASED_SQL`

The live plan is one irreversible step: version 1 is today’s final table
shapes, including `plugin_databases`. After 1.0, land new DDL in
`UNRELEASED_SQL` (not in `host_migration_plan()`). A release cut copies
that bucket into version N with a SHA-256 checksum and `introduced_in`.
Do not add a public plan version per PR.

Each `HostMigrationStep` stores:

- `canonical` (up SQL)
- `down: Option<&'static str>` — schema 1 is `None`
- `checksum` — SHA-256 of up (and down when present)
- `introduced_in`

`schema_migrations` columns are `version`, `checksum`, `app_version`,
`applied_at`. SQLite still sets `PRAGMA user_version` and writes the
metadata row so checksums are unified across backends.

### Apply on connect (fail closed)

| DB vs binary | Library open |
| --- | --- |
| DB `<` `SCHEMA_VERSION` | Snapshot, then apply remaining ups |
| DB `=` `SCHEMA_VERSION` | Verify checksums; mismatch refuses |
| DB `>` `SCHEMA_VERSION` | **Fail closed.** Message points at `bookclerk db downgrade` |

Never auto-downgrade on daemon or CLI library open.

### Snapshots

Before every up or down except empty-database create (`from_version == 0`):

- **SQLite:** `VACUUM INTO` under
  `$BOOKCLERK_FILES_DIR/snapshots/<utc>-pre-schema-<from>-to-<to>/`
- **Postgres:** SQL dump of host tables through the guest connection
- **D1:** Cloudflare REST export (`POST …/d1/database/{id}/export` + poll).
  Time Travel is unused.

Automatic snapshots do **not** include plugin databases and keep the last
five directories. `bookclerk db snapshot --path` writes a `.tar.gz` that
is never pruned. `--include-plugin-databases` copies SQLite
`plugin-databases/` files and dumps Postgres `pb_*` schemas; the host
does not migrate plugin DDL.

Jobs are not paused for snapshots. `VACUUM INTO` / dumps are
transactional; jobs and events already resume from durable receipts.

### Last-reversible CLI (Home Assistant walk)

`bookclerk db version|snapshot|restore|migrate|downgrade` uses
`connect_without_migrate` so an ahead schema can be inspected.

- `db migrate --to N` applies ups or reversible downs.
- `db downgrade` targets this binary’s `SCHEMA_VERSION`, walking `down`
  newest-first, and **stops at the last reversible version** when the next
  step has `down: None`. Non-zero exit if the database is still newer than
  the binary — restore the pre-upgrade snapshot.

The shipped plan is only v1, so `downgrade` on v1 is a no-op (restore a
snapshot to go back further). The walker is unit-tested with a synthetic
v1-irreversible / v2–v3-reversible plan.

### Out of scope

- Plugin-owned migration framework
- D1 Time Travel restore
- Auto-downgrade on daemon start
- Using semver as `schema_migrations.version`

## Consequences

- Fresh databases apply the frozen pack once. Historical V2–V29 fragments
  remain `#[cfg(test)]` archaeology for the D1 V27 FK rebuild tests only.
- Operators who skip a release still auto-upgrade on open (snapshot then
  ups). Operators who run an older binary against a newer DB restore a
  snapshot or run `bookclerk db downgrade` on a binary that still knows
  those steps.
- Editing frozen SQL after a release is a checksum mismatch, not a silent
  reshape.
