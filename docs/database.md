# Database plugins

Bookclerk stores library state (accounts, books, portal, discovery) in a
**database plugin**. Exactly one backend is active — unlike destinations, which
may fan out.

Default: local SQLite file (`$BOOKCLERK_FILES_DIR/library.db`). Optional:
Cloudflare D1 (remote SQLite via the Cloudflare HTTP API).

## ORM choice (Prisma-like maintainability)

Bookclerk previously used hand-written `rusqlite` SQL in
`bookclerk-library`. To keep schema and queries maintainable the way a
TypeScript app uses Prisma, we evaluated the Rust options:

| Option | What it is | Prisma-like DX | SQLite | Cloudflare D1 | Fit with Bookclerk |
| --- | --- | --- | --- | --- | --- |
| **SeaORM** | Async ActiveRecord-style ORM (entities, `ActiveModel`, relations, migrations) | Closest maintained match (TypeORM/Prisma ergonomics) | `sqlx-sqlite` | First-class `ProxyDatabaseTrait` (HTTP / Workers) | **Chosen** |
| **Diesel** | Sync compile-time query DSL | Strong types; steeper DSL; less “entity CRUD” feel | Excellent | `diesel-d1` is experimental, WASM Workers-only; HTTP API not ready | Strong for SQLite-only; weak for D1 from `bookclerk`/`bookclerkd` |
| **SQLx** | Compile-checked SQL, not an ORM | Far from Prisma | Yes | No first-class driver | Good toolkit, not the maintainability target |
| **prisma-client-rust** | Generated Prisma client for Rust | Most Prisma-like | Yes | Via Prisma ecosystem | **Unmaintained** — rejected |
| **Stay on rusqlite** | Thin SQLite wrapper | None | Native | Would be bespoke HTTP | Status quo; no ORM win |

### Why not Diesel?

Diesel’s compile-time guarantees are excellent, and Bookclerk’s current
`LibraryStore` is sync-friendly. D1 is the deal-breaker for an in-process
daemon/CLI:

- Official path for remote D1 from native binaries is the Cloudflare **HTTP**
  query API.
- SeaORM already supports custom backends through `Database::connect_proxy`.
- Diesel custom backends require
  `i-implement-a-third-party-backend-and-opt-into-breaking-changes` and a full
  `Backend`/`Connection` stack; `diesel-d1` today is Workers WASM + binding
  only.

### Why SeaORM 2

- Entity / `ActiveModel` workflow closest to Prisma Client usage patterns.
- Same `DatabaseConnection` for local SQLite and D1 (proxy).
- Migrations and schema stay SQL-first (compatible with existing
  `library.db` files) while queries move onto typed entities over time.

### `libsqlite3` alignment note

SeaORM’s `sqlx-sqlite` driver needs `libsqlite3-sys` `<0.38`. Upstream
`audible-rs` pins `rusqlite` 0.40 (`libsqlite3-sys` `^0.38`). Cargo cannot
link two `links = "sqlite3"` crates in one binary.

Bookclerk therefore:

- Pins workspace `rusqlite` to **0.37** and `rusqlite_migration` to **2.3** so
  they share `libsqlite3-sys` 0.35 with a single SQLite link.
- Vendors `audible-rs` under [`third_party/audible-rs`](../third_party/audible-rs)
  with only that dependency bump (see `BOOKCLERK_PATCH.md` there).
- Uses SeaORM’s **`proxy`** backend for both local SQLite (rusqlite wrapper) and
  D1 (HTTP). SeaORM 2.0’s `sqlx-sqlite` feature is not enabled: it currently
  fails to compile against `sea-query` 1.0.x (`Value` payload boxing change).

## Configuration

```toml
[database]
# Active plugin: "sqlite" (default) | "d1" | "postgres"
plugin = "sqlite"

[database.sqlite]
# Relative to BOOKCLERK_FILES_DIR, or absolute. Default: library.db
# path = "library.db"

[database.d1]
account_id = "your-cloudflare-account-id"
database_id = "your-d1-database-uuid"
# Token is read from BOOKCLERK_D1_API_TOKEN or CLOUDFLARE_API_TOKEN (env-only).
# api_base = "https://api.cloudflare.com/client/v4"

[database.postgres]
# Standard postgres:// connection URL. The URL contains credentials —
# prefer url_file or BOOKCLERK_DATABASE_POSTGRES_URL (registered for redaction).
# url = "postgres://user:password@localhost:5432/bookclerk"
# url_file = "/run/secrets/postgres_url"   # path to a file containing the URL
```

The Cloudflare API token is supplied via the environment only
(`BOOKCLERK_D1_API_TOKEN`, falling back to `CLOUDFLARE_API_TOKEN`); there is no
credentials file.

Environment overrides:

| Variable | Role |
| --- | --- |
| `BOOKCLERK_DATABASE_PLUGIN` | `sqlite`, `d1`, or `postgres` |
| `BOOKCLERK_DATABASE_SQLITE_PATH` | SQLite path override |
| `BOOKCLERK_D1_ACCOUNT_ID` | Cloudflare account id |
| `BOOKCLERK_D1_DATABASE_ID` | D1 database UUID |
| `BOOKCLERK_D1_API_TOKEN` / `CLOUDFLARE_API_TOKEN` | D1 API token |
| `BOOKCLERK_D1_API_BASE` | API base URL override |
| `BOOKCLERK_DATABASE_POSTGRES_URL` | Postgres connection URL (registered as secret) |
| `BOOKCLERK_DATABASE_POSTGRES_URL_FILE` | Path to file containing Postgres URL |

## Plugin kinds

Built-in backends are selected by `[database].plugin` (in-process), similar to
`[output.local]` / `[output.s3]`.

External `plugin.toml` may declare `kind = "database"` for discovery; host
loading of third-party database plugins is reserved for a follow-up (same
pattern as external output plugins).

## Postgres plugin

`plugin = "postgres"` connects to a PostgreSQL server using the sqlx-postgres
driver inside SeaORM. The connection URL is standard libpq-style:
`postgres://user:password@host:5432/dbname`.

Configuration (at least one of `url` or `url_file` is required):

```toml
[database]
plugin = "postgres"

[database.postgres]
# Option A: inline URL (registered as secret for log redaction)
url = "postgres://bookclerk:secret@db.example.com/bookclerk"

# Option B: file containing the URL (preferred for production)
url_file = "/run/secrets/postgres_url"
```

Or via environment:

```
BOOKCLERK_DATABASE_PLUGIN=postgres
BOOKCLERK_DATABASE_POSTGRES_URL=postgres://user:pass@host/db
# Or:
BOOKCLERK_DATABASE_POSTGRES_URL_FILE=/run/secrets/postgres_url
```

**Schema migrations**: Fresh Postgres databases apply
[`latest_schema_postgres`](../crates/bookclerk-library/src/migrations.rs)
(the single greenfield DDL, with `BIGSERIAL` / `BIGINT` / `BYTEA` so integer
columns match the shared `i64` entities) and record version `1` in
`schema_migrations`. Every statement is `IF NOT EXISTS`, so re-application is a
no-op. Because the schema is greenfield rather than an incremental chain,
changing it means editing `latest_schema_postgres` and recreating (or manually
altering) an existing Postgres DB.

**Compiled features**: `sqlx-postgres` + `runtime-tokio-rustls` are enabled on
the `sea-orm` workspace dependency. `sqlx-sqlite` is intentionally excluded to
avoid the `libsqlite3-sys` link conflict with `rusqlite 0.37`.

## D1 caveats

- D1 is SQLite-compatible but accessed over HTTP; latency is higher than
  local `library.db`.
- Cloudflare D1 does not provide full interactive transaction semantics the
  way a local SQLite file does; prefer short statements and avoid relying on
  multi-statement ACID across the proxy.
- Schema migrations run via `apply_pending_migrations` (tracked in `schema_migrations`).

### LibraryStore status

The operator-facing [`LibraryStore`](../crates/bookclerk-library) is
**SeaORM-backed and async**: it holds a `DatabaseConnection` (proxy backend) and
every method is an `async fn` returning `Result<…>`. `open`, `open_in_memory`,
and `open_from_config` are async too; callers `.await` them.

CRUD runs on typed **SeaORM entities** (see
[`crate::entities`](../crates/bookclerk-library/src/entities)): one
`DeriveEntityModel` per table (`accounts`, `books`, `works`, `title_requests`,
`encrypted_secrets`, …). Reads use `Entity::find()` + `QueryFilter` / `QueryOrder`;
writes use `ActiveModel` insert/update. Upserts that previously relied on
`ON CONFLICT … COALESCE(…)` are load-then-merge in Rust so the same behavior
holds on every backend. All entity integer columns are `i64`, reals `f64`, blobs
`Vec<u8>`, and text (including RFC 3339 timestamps) `String`.

Connections come from `bookclerk_library::connect_from_config` / `connect_sqlite`
/ `connect_sqlite_memory` / `connect_d1` / `connect_postgres`, and
`LibraryStore::open_from_config` selects the right backend (SQLite, D1, or
Postgres) automatically.

The SQLite proxy ([`db::sqlite`](../crates/bookclerk-library/src/db/sqlite.rs))
returns **typed** SQL `NULL`s so SeaORM `Option<T>` decoding works: it reads each
column's declared type (rusqlite `decl_type`) and emits the matching
`Value::*(None)` (`BigInt(None)`, `Double(None)`, `Bytes(None)`, `String(None)`)
via [`db::typed_null`](../crates/bookclerk-library/src/db/mod.rs). D1 (JSON, no
type metadata) falls back to a column-name heuristic. rusqlite remains only for
the local SQLite proxy driver and the `rusqlite_migration` runner used on local
`library.db` files.

### Single greenfield schema

Bookclerk is greenfield: there is **one** current schema, not an ordered
M1…M10 chain. [`migrations.rs`](../crates/bookclerk-library/src/migrations.rs)
exposes `latest_schema_sqlite()` (also the single `rusqlite_migration` entry ⇒
`PRAGMA user_version = 1`) and `latest_schema_postgres()`. Every statement uses
`CREATE TABLE/INDEX IF NOT EXISTS`, so re-applying is idempotent. Tables:
`accounts`, `books`, `ignored_titles`, `saved_filters`, `portal_identities`,
`claim_tickets`, `portal_sessions`, `account_links`, `works`, `work_editions`,
`listening_progress`, `title_requests`, `embeddings`, `user_preferences`,
`encrypted_secrets`.

## Encrypted secrets

The `encrypted_secrets` table (part of the greenfield schema) enables DB-backed
storage for auth credentials, replacing file-based `Accounts/*.auth` files:

```sql
CREATE TABLE encrypted_secrets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,        -- 'source_auth' | 's3' | 'operator' | 'widevine' | 'd1' | …
  provider TEXT,             -- 'audible' | 'libro' | 'chirp' | 'graphicaudio' | null
  account_id TEXT,           -- per-provider account stem or null
  name TEXT NOT NULL,        -- file-stem equivalent
  format TEXT NOT NULL,      -- 'audible-rs-auth' | 'json' | 'json-encrypted'
  ciphertext BLOB NOT NULL,  -- raw or encrypted payload
  kdf_algorithm TEXT,        -- 'argon2id' or null for plaintext
  kdf_salt BLOB,
  kdf_m_cost INTEGER,        -- 65536 (64 MB)
  kdf_t_cost INTEGER,        -- 3
  kdf_p_cost INTEGER,        -- 1
  cipher_algorithm TEXT,     -- 'xchacha20poly1305' or null
  cipher_nonce BLOB,         -- 24-byte XChaCha20 nonce
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(kind, provider, account_id, name)
);
```

### SecretStore API

```rust
use bookclerk_library::secrets::{
    SecretStore, EncryptedSecretRecord, secret_kind,
    encrypt_secret, decrypt_secret,
};

// Upsert / get / list / delete via SecretStore wrapper or standalone fns:
let store = SecretStore::new(&db);
store.upsert(&record).await?;
let secret = store.get("source_auth", Some("audible"), Some("alice"), "alice.audible.auth").await?;
let all = store.list(secret_kind::SOURCE_AUTH).await?;
store.delete("source_auth", Some("audible"), Some("alice"), "alice.audible.auth").await?;
```

### Encryption

- Audible auth (`source_auth / audible`) is stored as `format="audible-rs-auth"` with the raw
  envelope bytes unchanged — the audible-rs layer already handles Argon2id + XChaCha20-Poly1305.
- Other source credentials (`source_auth / libro`, `/ chirp`, `/ graphicaudio`) are serialized
  as JSON and encrypted with Argon2id + XChaCha20-Poly1305 when `BOOKCLERK_AUTH_PASSWORD` is set,
  or stored as plaintext JSON with a warning if none is set.
- Widevine CDM blobs (`widevine / audible`) are stored verbatim; the blob's own L3 protection
  is sufficient.
- The master password comes from `BOOKCLERK_AUTH_PASSWORD` (env-only bootstrap —
  never stored in the DB).

### Bootstrap secrets stay outside the DB

These are required to open the DB or derive the master key and cannot be stored here:
- `BOOKCLERK_AUTH_PASSWORD`
- `BOOKCLERK_DATABASE_POSTGRES_URL` / `BOOKCLERK_D1_API_TOKEN` — DB connection bootstrap
- `BOOKCLERK_OPERATOR_TOKEN` — operator API key bootstrap
- `config.toml` (remains on disk)

> **No `Accounts/` directory for secrets.** All runtime credentials (Audible, Libro.fm,
> Chirp, GraphicAudio, Widevine CDM, and S3 destination keys) are stored in
> `encrypted_secrets`. S3 still accepts env override (`AWS_ACCESS_KEY_ID` /
> `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`) and falls back to the AWS
> SDK default provider chain when no DB row is present. Bookclerk no longer creates
> or reads an `Accounts/` directory.
