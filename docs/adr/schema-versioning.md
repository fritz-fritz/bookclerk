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

On disk, `schema_migrations` stores `namespace`, `state` (`unreleased` |
`frozen`), `version`, `checksum`, `app_version`, and `applied_at`. Primary
key is `(namespace, state, version)`. Host library and Bookclerk-owned
binding bootstrap use namespace `bookclerk`. Plugin-owned evolution in a
binding uses the plugin id. For unreleased rows, `version` **is** the frozen
base (`0` before any freeze). Host logic keys off `state` plus that base.
`PRAGMA user_version` is a frozen-version cache only.

`bookclerk db version` prints `uninitialized`,
`unreleased@base<n>+<checksum>`, or `frozen@<version>+<checksum>`.

### Unreleased pack until a real release cut

[`host_migration_plan()`](../../crates/bookclerk-library/src/migrations.rs)
is **empty**. Live schema lives in `unreleased_ops` (already-separated
`MigrationOp`s). Fresh databases apply those ops (frozen ups, then the
current unreleased list) and persist
`Unreleased { base_version: SCHEMA_VERSION, checksum }`. Joined scripts
(`current_canonical_schema()`, `unreleased_sql()`) are derived for
diagnostics, export, and `execute_batch` fixtures; they are not re-parsed
to recover statement boundaries.

`current_canonical_schema()` is **not** permanently equal to
`unreleased_sql()`. Today the frozen plan is empty, so they coincide. After a
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

A future freeze that copies `unreleased_ops` into
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
| SQLite | Read transaction over admitted tables; paged `ORDER BY` rows as `DbValue` | Restore transaction + `PRAGMA defer_foreign_keys` (never toggle `foreign_keys` on the pool); `PRAGMA foreign_key_check` before commit; `sqlite_sequence` high-water |
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

Crash-safe object write: unique same-directory temp → fsync the file →
no-clobber hard-link install → fsync the parent directory. Concurrent
writers that lose the install verify the winner's object. Incomplete
staging remains invisible. List/restore ignore unpublished work. Object
publication and GC/prune take an exclusive repository lock (`backups/.lock`)
so in-progress objects are not collected before the manifest commit.
Archive extraction refuses `..`, absolute paths, symlinks, hardlinks, and
other non-file/directory tar types, and enforces entry-count, per-entry,
total-expanded, and gzip-decoded stream budgets. Stored objects declare
uncompressed length in the envelope and refuse gzip expansion past that
cap. Temporary unpack directories are removed on success and failure.

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
markers restore as ordinary rows; the plugin may issue host-mediated
BookclerkSQL in its binding after startup. Restore does not run plugin
migrations.
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

### Host-mediated plugin schema (not a plugin migration framework)

Plugin bindings may change schema only as **admitted BookclerkSQL** inside the
plugin’s binding namespace. Ordinary `execute` uses
`GuestSqlPolicy::binding_owned` (DML/query). Frozen plan apply uses
`GuestSqlPolicy::binding_migration` (bounded `CREATE`/`DROP IF [NOT] EXISTS`).
The host authorizes, typechecks, and records binding `SchemaState` in that
binding’s own `schema_migrations` table (same row shape as the library).

Host-owned binding bootstrap is [`binding_bootstrap_ops`](../../crates/bookclerk-library/src/migrations.rs)
(`Schema` ops: receipts + SQL catalog). `BINDING_SCHEMA_VERSION = 0` until a
binding freeze. Connect:

| Binding | Open |
| --- | --- |
| `Uninitialized` | Apply bootstrap ops + unreleased marker (one atomic unit; retry uniqueness / unavailable after re-read) |
| Matching `Unreleased { base_version, checksum }` | No-op |
| Mismatched checksum / unexpected frozen | **Fail closed** (restore or drop the binding) |

There is **no** backend-native escape hatch (`pg_dump`, `VACUUM INTO`, D1 REST
migrate, sqlite `.dump`). Plugin-owned schema evolution uses the **same**
migration engine as host library and binding bootstrap: an ordered
[`MigrationPlan`](../../crates/bookclerk-library/src/migrations/plan.rs) of
proven BookclerkSQL `up` / optional `down` ops. Plugin plans ship as
immutable package metadata (`plugin.toml` `migration_plan = "migrations.toml"`;
SHA-256 of the raw installed file). Namespace in the binding ledger is always
the plugin id — never `BINDING_SCHEMA_VERSION` and never a TOML override to
`bookclerk`. Ordinary binding `execute` is query/DML;
`CREATE`/`ALTER`/`DROP` is admitted only in a migration execution context.

Restore writes captured logical schema/data/migration ledger **exactly** and
does **not** run migrations inside the destructive restore transaction. After
restore completes, ordinary binding open may walk the installed plan forward;
if the installed plugin cannot understand restored state, open fails closed.

Host schema and plugin schema are separate apply units. Each frozen/unreleased
**apply unit** is one atomic `ExecuteRequest` (serialization slot + ops +
marker). If an adapter cannot perform a schema transition atomically, it must
not advertise the capability (D1 already uses HTTP batch; backup flags stay
off).

### Migration ops (still unreleased)

```text
HostMigrationStep { version, introduced_in, steps, down }
MigrationOp = Schema(canonical stmt) | Data(canonical stmt)
```

The **op list is the source of truth**. Human-readable SQL
(`unreleased_sql()`, `current_canonical_schema()`, `HostMigrationStep::up_sql`)
is derived by joining already-separated statements with `;\n` for
diagnostics/export/tests. Apply, checksum, and backup walk the ordered ops.
Each `MigrationOp` is exactly one BookclerkSQL statement; `Schema` must be
admitted DDL and `Data` admitted DML, both proven through the SQL-v1
typechecker before adapter execution. A packer failure on static or imported
migration SQL fails closed (never checksumed as one opaque statement).

`host_migration_plan()` stays **empty**. Live schema is [`unreleased_ops`](../../crates/bookclerk-library/src/migrations.rs)
(mostly `Schema`, plus seed `Data` inserts). Checksums hash the
**length-prefixed ordered statement list**, not a joined script. CLI downgrade
applies `down` only when every step has it; otherwise restore a backup.

Invariants (locked with synthetic plans, not a v1 freeze):

- Integer version order; host vs plugin namespaces; fail closed on
  contradictory `schema_migrations` rows.
- Retry uniqueness / duplicate-object / unavailable after re-read; matching
  version+checksum is success; FK / CHECK / NOT NULL are not races.
- Crash: marker not visible ⇒ retry the same unit.
- Forward-only on library/binding open; rolling binaries fail closed on
  unknown newer frozen or mismatched checksums.
- Multi-node: portable `db_serialization_slots` keyed `schema:{namespace}`
  (no `pg_advisory_xact_lock` in host). Stale sessions re-check plugin
  schema at execute boundaries.
- Backup capture records plugin namespace identity/version/checksum; restore
  into a newer app stays fail-closed until walk/migrate is an ordinary open
  (not inside the restore transaction).
- Oldest frozen host revision is derived from `host_migration_plan()`
  (`None` while the plan is empty). There is no independent
  `MIN_SUPPORTED_SCHEMA_VERSION` constant.

### Last-reversible CLI

`bookclerk db version|backup|restore|migrate|downgrade` uses
`connect_without_migrate`.

With an empty frozen plan, schema-version downgrade is a no-op. Time-based
**restore** is how operators move independently of schema revision.

### Out of scope

- Declaring production schema v1
- D1 Time Travel / portable PITR / canonical change journaling
- Transactional atomicity across library DB + independent plugin DBs
- Using semver as `schema_migrations.version`

### Shared plugin migration engine

Host library frozen steps, Bookclerk-owned binding bootstrap, and plugin-owned
binding evolution share one apply engine (`MigrationPlan` / `MigrationStep` /
`PlanOp::{Schema,Data}`). Plugin plans are frozen steps starting at version 1;
they never use Unreleased and never reuse `BINDING_SCHEMA_VERSION`.

| Concern | Rule |
| --- | --- |
| Per-plugin / per-binding schema version | Namespaced `schema_migrations` (`bookclerk` vs plugin id). |
| Ordered progression | Immutable companion `migrations.toml`; host proves BookclerkSQL. |
| DDL outside a plan | Fail closed. Ordinary binding execute is DML/query only. |
| Multi-node concurrency | `lock_serialization_slot("schema:{namespace}")`; re-read under the fence. |
| Retries / interrupted recovery | Uniqueness / unavailable after re-read; matching version+checksum ⇒ success; absent marker ⇒ retry same step; contradictory checksum/version ⇒ fail closed. |
| Upgrade | Installed plan newer than stored state walks forward. Stored state newer than the installed plan ⇒ fail closed. |
| Downgrade | Never automatic. Explicit only when every traversed step has a validated `down`; otherwise restore a recovery point. |
| Restore | Restore captured ledger exactly. Do not migrate inside the restore transaction. Next ordinary open may forward-walk. |
| Stale session | Binding execute re-checks expected plugin schema revision/checksum. |

Acceptance includes crash/retry, two nodes racing the same upgrade, old-session
fencing, newer-schema/older-plugin rejection, reversible and irreversible
downgrade, and backup → cross-adapter restore → forward-migrate (SQLite /
Postgres; D1 does not advertise backup flags).

## Consequences

- Fresh databases apply the unreleased pack once and record
  `unreleased@base0+<checksum>` until a freeze exists.
- Editing `unreleased_ops` against an existing development DB is a hard
  fail + reset, not a silent reshape.
- There is no incremental V2–V29 chain and no `migrations_legacy` module.
  Current-schema FK/UNIQUE coverage lives on the greenfield pack.
- A later freeze can concatenate frozen ups + a new unreleased bucket
  through `current_canonical_schema()` without unwinding callers.
- Schema-apply retries uniqueness / duplicate-object races and transient
  unavailability after re-reading durable state. Foreign key, CHECK, and
  NOT NULL failures are not schema-apply races.
