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
# Active plugin: "sqlite" (default) | "d1"
plugin = "sqlite"

[database.sqlite]
# Relative to BOOKCLERK_FILES_DIR, or absolute. Default: library.db
# path = "library.db"

[database.d1]
account_id = "your-cloudflare-account-id"
database_id = "your-d1-database-uuid"
# Prefer a token file under Accounts/ (same pattern as S3 auth files).
# credentials_file = "Accounts/default.d1.auth"
# Or set BOOKCLERK_D1_API_TOKEN / CLOUDFLARE_API_TOKEN.
# api_base = "https://api.cloudflare.com/client/v4"
```

D1 credentials JSON (`Accounts/*.d1.auth`):

```json
{
  "api_token": "…",
  "label": "prod"
}
```

Environment overrides:

| Variable | Role |
| --- | --- |
| `BOOKCLERK_DATABASE_PLUGIN` | `sqlite` or `d1` |
| `BOOKCLERK_DATABASE_SQLITE_PATH` | SQLite path override |
| `BOOKCLERK_D1_ACCOUNT_ID` | Cloudflare account id |
| `BOOKCLERK_D1_DATABASE_ID` | D1 database UUID |
| `BOOKCLERK_D1_API_TOKEN` / `CLOUDFLARE_API_TOKEN` | API token |
| `BOOKCLERK_D1_CREDENTIALS_FILE` | Path to `*.d1.auth` |
| `BOOKCLERK_D1_API_BASE` | API base URL override |

## Plugin kinds

Built-in backends are selected by `[database].plugin` (in-process), similar to
`[output.local]` / `[output.s3]`.

External `plugin.toml` may declare `kind = "database"` for discovery; host
loading of third-party database plugins is reserved for a follow-up (same
pattern as external output plugins).

## D1 caveats

- D1 is SQLite-compatible but accessed over HTTP; latency is higher than
  local `library.db`.
- Cloudflare D1 does not provide full interactive transaction semantics the
  way a local SQLite file does; prefer short statements and avoid relying on
  multi-statement ACID across the proxy.
- Schema migrations still run at open (same SQL as local SQLite).

### LibraryStore status

The operator-facing [`LibraryStore`](../crates/bookclerk-library) query API is
still **rusqlite-backed** on the `sqlite` plugin (zero behavior change for the
default path). SeaORM connections are available via
`bookclerk_library::connect_from_config` / `connect_sqlite` / `connect_d1` for
pings and the upcoming entity migration. Selecting `plugin = "d1"` for full
library operations will land once `LibraryStore` methods run through SeaORM.
