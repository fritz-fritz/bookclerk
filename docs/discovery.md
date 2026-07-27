# Discovery, recommendations, and request queue

Bookclerk’s discovery stack turns owned-library metadata, optional catalog
enrichment, and listening signals into **next-purchase recommendations** and a
**title request queue**. Operator-facing surfaces ship first; the schema is
ready for Connect-portal identities later.

## Goals

1. **Recommend** titles the household does not yet own (or has not finished).
2. **Discover** similar works from library metadata + embeddings.
3. **Queue requests** (“please buy this”) with optional preferred storefront.
4. **Suggest where to buy** via store catalog search (Audible public catalog,
   Libro.fm explore) when an ISBN/ASIN/title is known.
5. Stay fit for a **cheap VPS** (≈1 vCPU, 1–2 GB RAM): no separate vector
   database process; one small ONNX embedding model loaded on demand.

## Architecture

```text
┌─────────────┐  scan/enrich   ┌──────────────┐  works+editions  ┌─────────────┐
│ Sources     │ ─────────────► │ library.db   │ ───────────────► │ embeddings  │
│ + OpenLib   │                │ books/works  │   (SQLite BLOB)  │ (MiniLM-L6) │
└─────────────┘                └──────┬───────┘                  └──────┬──────┘
                                      │                                 │
┌─────────────┐  progress sync        │        recommend                │
│ ABS users   │ ─────────────────────►│◄────────────────────────────────┘
└─────────────┘                       ▼
                               ┌──────────────┐     purchase hints
                               │ discover     │ ────────────────► Audible / Libro
                               │ engine       │                    public APIs
                               └──────┬───────┘
                                      │
                               ┌──────▼───────┐
                               │ title_requests│  (identity_id nullable)
                               └──────────────┘
```

### Phasing (operator first, portal later)

| Phase | Audience | Notes |
| --- | --- | --- |
| **Now** | Operator (CLI + daemon GUI) | `identity_id` null = operator; ABS progress keyed by `external_user_id` |
| **Later** | Connect portal users | Bind `title_requests.identity_id` / listening rows to `portal_identities` |

## Data model (SQLite)

New columns on `books` (durable enrichment):

- `description`, `language`, `cover_url`, `subjects`
- `enrich_source`, `enrich_confidence`, `enrich_updated_at`

New tables:

| Table | Role |
| --- | --- |
| `works` | Canonical work (preferred ASIN/ISBN, subjects, Open Library id) |
| `work_editions` | Maps `book_uuid` → `work_id` |
| `listening_progress` | ABS (or future) progress snapshots |
| `title_requests` | Request queue (`open` / `approved` / `acquired` / `rejected` / `cancelled`) |
| `embeddings` | `target_kind` + `target_id` + model + f32 LE vector BLOB |
| `recommendation_snapshots` | Optional cached payload per identity |

Vectors live in SQLite. Library-scale cosine search is brute-force in process
(thousands of titles), which avoids Qdrant/LanceDB memory and ops cost.

## Enrichment

1. **Audible / Audnexus** (existing) — now **persists** description, language,
   cover URL, and subjects/categories already scored.
2. **Open Library** — ISBN / title+author fallback for subjects, description,
   and cover when Audible enrichment is missing or weak.
3. **WorldCat** — not implemented (API key + ToS). Hook documented under
   `[discovery]` for a future optional provider.

Post-scan: Audible enrich → Open Library fill-gaps → rebuild works → embed
dirty works (when embeddings enabled).

## Listening signals (AudioBookshelf)

When `[integrations.audiobookshelf]` is configured, Bookclerk periodically
(or on CLI `discover sync-listening`):

1. `GET /api/users`
2. `GET /api/users/{id}` (includes `mediaProgress`)
3. Optionally `GET /api/items/{libraryItemId}` for title/ASIN/ISBN metadata
4. Upsert `listening_progress`; best-effort match to `book_uuid`

Signals used by the recommender: finished titles, in-progress (high progress),
recency (`lastUpdate`), and linked portal identity when present.

## Embeddings (small / VPS-friendly)

Default build uses a **local-hash** 384-d embedder (no download, negligible RAM).
Optional Cargo feature `onnx-embeddings` enables quantized MiniLM-L6-v2 via
`fastembed` / ONNX Runtime (~22 MB on disk, ~50 MB RAM, 1 intra-thread).

| Setting | Default |
| --- | --- |
| Model (default build) | `local-hash-v1` |
| Model (with `onnx-embeddings`) | `all-minilm-l6-v2-q` |
| Cache | `$BOOKCLERK_FILES_DIR/models/` |
| Lifecycle | Load for batch embed / query; drop afterward |

ONNX prebuilt binaries currently need **glibc ≥ 2.38**. On older hosts (e.g.
Debian 12 / glibc 2.36) Bookclerk falls back to `local-hash-v1` automatically.
Enable ONNX with:

```bash
cargo build -p bookclerk-cli -p bookclerkd --features bookclerk-discover/onnx-embeddings
```

Disable embeddings entirely with `discovery.embeddings_enabled = false`
(heuristics-only mode).

Embedded text: title, authors, narrators, series, categories/subjects,
description (truncated). Re-embed when `text_hash` changes.

## Recommendation ranking

Candidates are **not owned** (or not acquired) works / catalog hits, scored by:

1. **Series gap** — next index in a series you own / finished
2. **Same author / narrator / subject** overlap with liked signals
3. **Embedding similarity** to finished / highly rated / recently listened works
4. **Request boost** — open requests matching a candidate
5. **Purchase availability** — Audible ASIN and/or Libro ISBN explore hit

Each recommendation includes `reasons[]` and optional `purchase_hints[]`
(`source`, `product_id`, `title`, `url` when known).

## Request queue

Operator CLI / API:

- create / list / update status / cancel
- optional `asin` / `isbn` / `preferred_source` / notes
- approve → optional acquire when product id resolves to an owned-account store

Portal users later submit with `identity_id` set; operators triage the same table.

## Configuration

```toml
[discovery]
embeddings_enabled = true
# quantized MiniLM-L6-v2 — keep for 1–2 GB VPS hosts
embedding_model = "all-minilm-l6-v2-q"
embed_intra_threads = 1
openlibrary_enabled = true
# worldcat_enabled = false  # reserved; needs API key
listen_sync_interval_minutes = 60
recommend_limit = 20
```

Environment overrides use the `BOOKCLERK_DISCOVERY_*` prefix (see
[configuration.md](configuration.md)).

## Surfaces

| Surface | Commands / routes |
| --- | --- |
| CLI | `bookclerk discover recommend`, `embed`, `sync-listening`, `request …` |
| Daemon | `GET /api/discover/recommendations`, `CRUD /api/discover/requests` |
| GUI | Discover page (recommendations + request queue) |

## Non-goals (this iteration)

- Spatial/Atmos or L1 CDM recommendations
- Paid WorldCat / Goodreads / Hardcover providers
- Real-time collaborative filtering across households
- A long-running vector DB sidecar
