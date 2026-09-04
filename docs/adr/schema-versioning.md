# ADR: Schema state, unreleased development packs, and backups

- **Status:** Accepted
- **Date:** 2026-09-03
- **Related:** [SQL database contract](sql-database-contract.md),
  [database.md](../database.md), [migration.md](../migration.md)

## Context

Bookclerk’s library schema used to grow as an incremental chain (named
fragments V2–V29). No production databases exist, so that chain is not an
upgrade path. Developers reset local DBs. The host still auto-applies the
current development pack on library open.

Integer `0` cannot distinguish a fresh SQLite database from an applied
development schema (`PRAGMA user_version` defaults to 0). A future release
cut must not treat an existing unreleased database as an empty predecessor
to frozen v1.

`SQL_CONTRACT_VERSION = 1` / SQL-v1 is the **SQL grammar/ABI**, not a
library schema freeze. There is **no** frozen production schema v1.

## Decision

### Explicit schema state

Internal and CLI state is not an integer identity:

```text
Uninitialized
Unreleased { base_version, checksum }
Frozen { version, checksum }
```

`SCHEMA_VERSION` is the highest **frozen** revision this binary knows. Today
it is `0` because `host_migration_plan()` is empty — that means “there are
no frozen schema revisions,” **not** “this database is at schema zero.”

Unreleased always records its frozen base explicitly. Today that is
`Unreleased { base_version: 0, checksum }` (pre-v1 development). After a
future v1 freeze:

- a release database may be `Frozen { version: 1, checksum }`
- a development database with additional unreleased DDL becomes
  `Unreleased { base_version: 1, checksum }`

Malformed / missing checksum / contradictory markers are a failure class,
not a state to continue from. Do not infer uninitialized or unreleased from
integer `0` alone.

On disk, `schema_migrations` stores `state` (`unreleased` | `frozen`),
`version`, `checksum`, `app_version`, and `applied_at`. For unreleased rows,
`version` **is** the frozen base (`0` before any freeze). Host logic keys
off `state` plus that base. `PRAGMA user_version` is a frozen-version cache
only.

`bookclerk db version` prints `uninitialized`,
`unreleased@base<n>+<checksum>`, or `frozen@<version>+<checksum>`.

### Unreleased pack until a real release cut

[`host_migration_plan()`](../../crates/bookclerk-library/src/migrations.rs)
is **empty**. Live DDL lives in `UNRELEASED_SQL`. Fresh databases apply
[`current_canonical_schema()`](../../crates/bookclerk-library/src/migrations.rs)
(frozen ups + current unreleased) and persist
`Unreleased { base_version: SCHEMA_VERSION, checksum }`.

`current_canonical_schema()` is **not** permanently equal to
`UNRELEASED_SQL`. Today the frozen plan is empty, so they coincide. After a
freeze it is concatenated frozen ups plus whatever is again unreleased.

There is **no** production schema v1 freeze in this tree.

Connect transitions:

| Database | Library open |
| --- | --- |
| `Uninitialized` | Apply frozen canonical base known to this binary, then the current unreleased bucket; persist current state |
| Matching `Unreleased { base_version, checksum }` | No-op (`base_version` must equal this binary’s `SCHEMA_VERSION`) |
| Mismatched unreleased checksum or frozen base | **Fail closed** (`cargo reset --yes`) unless a later explicit exact-checksum promote exists |
| `Frozen { version, checksum }` | Verify frozen checksums; apply remaining frozen steps; then apply the current unreleased bucket if one exists. Resulting state is `Unreleased { base_version: latest_frozen, checksum }` when the bucket is non-empty |
| `Frozen` newer / unknown / checksum mismatch | **Fail closed** |
| Pre-state-machine DB (`user_version` ≫ 0, no state row) | Unsupported / malformed; reset |

A future freeze that copies `UNRELEASED_SQL` into
`HostMigrationStep { version: 1 }` must **not** apply v1 ups on top of an
existing `Unreleased { base_version: 0, … }` database just because both
historically used integer zero. Default: documented development reset.
Optional later promote: exact-checksum marker rewrite only.

Never auto-downgrade on daemon or CLI library open.

### Backups (recovery points)

A **recovery point** is one complete logical database state at a specific
time. The physical repository may reuse immutable canonical objects from
earlier recovery points. Restore never replays older manifests (no chained
incrementals).

`DbCapabilities.consistentBackupRead` means the adapter can expose one
stable logical state while Bookclerk reads schema, rows, and identity.
`DbCapabilities.atomicUnitRestore` means destructive restore of one logical
unit does not leave that unit partially replaced after an ordinary failure.
Orchestration keys on these flags, never on sqlite/postgres/d1 plugin ids. The host owns IDs,
timestamps, manifests, retention, and lookup. The durable artifact is
canonical Bookclerk content (admitted schema + portable `DbValue` chunks),
not VACUUM / `pg_dump` / D1 REST / native pages.

| Adapter | Consistent capture | Complete unit restore |
| --- | --- | --- |
| SQLite | Read transaction over admitted tables; paged `ORDER BY` rows as `DbValue` | Transaction + `PRAGMA foreign_keys` off during replace; `sqlite_sequence` high-water |
| Postgres | `REPEATABLE READ` transaction | Transactional replace; `bookclerk_identity` high-water (not native sequences) |
| D1 | **Not advertised.** Sequential HTTP is not a consistent image. | **Not advertised.** Sequential REST DROP/INSERT is not complete per-unit replacement. |

A recovery point never silently omits an expected table or fabricates SQL
`NULL` for an unrepresentable value. Restore eligibility does not require the
same adapter that captured the backup.

Missing backup capability fails backup/restore closed. Restore never
merges. Restore does **not** auto-migrate the host library and does **not**
run plugin-owned migrations. Integrity (manifest parse, supported format,
every object digest, admitted schema, typed cells) is verified **before**
any destructive action. The manifest is published only after every
referenced object exists.

Crash-safe object write: temp file → fsync-equivalent rename. Incomplete
staging remains invisible. List/restore ignore unpublished work. Archive
extraction refuses `..`, absolute paths, symlinks, hardlinks, and
other non-file/directory tar types. Temporary unpack directories are
removed on success and failure.

Retention prunes automatic `pre-migrate` recovery points only. **Never** prune
`manual` backups. Reachability GC deletes objects no retained manifest
references. `bookclerk db backup create` is manual;
`bookclerk db backup list` lists by time; `backup verify` / restore accept
path, id, or timestamp. Duplicate / ambiguous lookup fails rather than picking
arbitrarily.

Skip capture only for `Uninitialized`. Unreleased databases with data are
backup-capable. Library schema at capture is the schema that matches the
recorded `SchemaState` (frozen vN under a newer binary exports vN, not the
latest pack).

`--include-plugin-databases` enumerates `plugin_databases` registry rows
(not the `plugin-databases/` directory). Each logical unit is
`(plugin_id, binding)`, opened through the active adapter session. A failed
unit fails the requested bundle. Units are replaced individually; a bundle is
not one transaction across independent databases. Plugin schema/version
markers restore as ordinary rows; the plugin may migrate after startup.
Library-only restore preserves the target registry. Included restore rebinds
registry rows to the target adapter’s physical placement (never source
`unit_ref`).

### Future portable PITR (not implemented)

A later design may journal canonical Bookclerk transactions at commit
boundaries and restore `base recovery point + committed change segments`.
Required future invariants would include: journal entry atomic with DB
commit; transaction boundaries; all mutations including DDL and plugin DDL
without stealing plugin migration ownership; replay idempotency; retention
tied to the oldest retained base recovery point. This repository’s object
store is intended to remain the base layer for that work. Do not emit a
change journal until that design lands.

### Last-reversible CLI

`bookclerk db version|backup|restore|migrate|downgrade` uses
`connect_without_migrate`.

With an empty frozen plan, schema-version downgrade is a no-op. Time-based
**restore** is how operators move independently of schema revision.

### Out of scope

- Declaring production schema v1
- Plugin-owned migration framework
- D1 Time Travel / portable PITR / canonical change journaling
- Transactional atomicity across library DB + independent plugin DBs
- Using semver as `schema_migrations.version`

## Consequences

- Fresh databases apply the unreleased pack once and record
  `unreleased@base0+<checksum>` until a freeze exists.
- Editing `UNRELEASED_SQL` against an existing development DB is a hard
  fail + reset, not a silent reshape.
- There is no incremental V2–V29 chain and no `migrations_legacy` module.
  Current-schema FK/UNIQUE coverage lives on the greenfield pack.
- A later freeze can concatenate frozen ups + a new unreleased bucket
  through `current_canonical_schema()` without unwinding callers.
- Schema-apply retries uniqueness / duplicate-object races and transient
  unavailability after re-reading durable state. Foreign key, CHECK, and
  NOT NULL failures are not schema-apply races.
