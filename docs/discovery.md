# Discovery, recommendations, and request queue

Bookclerk’s discovery stack turns **local taste** (owned library + listening)
into **unowned storefront candidates**, then ranks those with embeddings and
heuristics. Operator-facing surfaces ship first; the schema is ready for
Connect-portal identities later.

## Goals

1. **Discover unowned titles** from storefront catalogs (Libro related books,
   Audible public search / series ASIN, Chirp GraphQL related/series/author/
   deals, GraphicAudio Magento series + related), not only from the owned library.
2. **Evaluate** those candidates against local ownership, listening, ratings,
   and embeddings.
3. **Queue requests** (“please buy this” / wishlist) with optional preferred storefront —
   including one-click from Discover cards.
4. **Suggest where to buy** once a candidate is known.
5. Stay fit for a **cheap VPS** (≈1 vCPU, 1–2 GB RAM): no separate vector DB;
   embeddings score remote candidates locally.

## Candidate generation (storefronts first)

```text
 local taste seeds          storefront expansion           local scoring
 (finished / rated /   →   Libro related_audiobooks   →   filter owned
  listening)                Audible author / series         embed similarity
                            ASIN / (opt) narrator           author/series boost
                            Chirp related / series /        purchase hints
                            author / catalog / deals
                            GraphicAudio Magento
                            related + series + search
```

Owned library rows are **seeds**, not the candidate pool. Open title requests
merge into the ranked list as high-priority operator intent.

Chirp and GraphicAudio expansion prefer seeds already owned on those sources
(Chirp product id → `relatedAudiobooks`; Magento product id → related block +
series page). Series / author signals from any seed can still query Chirp or
Magento when the catalog has a match. GraphicAudio series-set SKUs are
**included by default**; set `exclude_graphicaudio_series_sets = true` to drop
them. Chirp top/free deals are pulled once per recommend run (unowned only).

Listening taste is **optional** and **provider-agnostic**. Integrations that
support listening sync (Audiobookshelf today; third-party plugins via capability
`sync_listening`) write into the shared `listening_progress` table. Ranking never
calls those adapters — if no listening data is present, or you pass
`--no-listening` / `?no_listening=1`, discovery still runs on owned-library taste
alone. Scope with optional `external_user_id` / `?user=` and/or
`--listening-provider` / `?listening_providers=` for multi-user or multi-integration
libraries.

## Open Library (compliance)

Used only for **low-volume metadata gap-fill** on owned rows (subjects /
description / cover), never as a bulk catalog backend.

Per [Open Library API guidelines](https://openlibrary.org/developers/api):

- Identify with `User-Agent: Bookclerk/<ver> (<email>; …)` via
  `discovery.openlibrary_contact_email`
- Space requests (~400 ms); cap `openlibrary_max_requests_per_run` (default 25)
- Skip rows already enriched (`enrich_source = openlibrary`) — provenance cache
- For bulk data, use [monthly dumps](https://openlibrary.org/data), not the API

WorldCat remains reserved (API key + ToS).

## Embeddings (small / VPS-friendly)

Default build uses a **local-hash** 384-d embedder (no download, negligible RAM)
to score storefront candidate text against a centroid of finished/liked works.
Optional Cargo feature `onnx-embeddings` enables quantized MiniLM-L6-v2 via
`fastembed` / ONNX Runtime (~22 MB on disk, ~50 MB RAM, 1 intra-thread).

ONNX prebuilt binaries currently need **glibc ≥ 2.38**. On older hosts Bookclerk
falls back to `local-hash-v1`. Enable ONNX with:

```bash
cargo build -p bookclerk-cli -p bookclerkd --features bookclerk-discover/onnx-embeddings
```

## Recommendation ranking

Candidates are **unowned** storefront hits (plus open requests), scored by:

1. Storefront origin (related / series / author / catalog search)
2. **Series completion** — own some of a series → boost unowned siblings; prefer
   the next index; stronger when the series is actively listened to, and
   stronger still when multiple books in that series have listening activity.
   Listening weight is driven by **absolute hours heard** (soft-saturated), with
   percent complete only a secondary bonus — so 50% of a 30‑hour book outweighs
   50% of a 3‑hour book. Finished titles still get a completion bump on top of
   hours listened.
3. Overlap with liked authors
4. Embedding similarity to finished / highly rated / recently listened works
5. Open request boost
6. Purchase hints from the proposing store (and cross-store lookup when needed)

## Request queue

Operator CLI / API create / list / update status. Portal users later submit with
`identity_id` set; operators triage the same table.

## Configuration

```toml
[discovery]
embeddings_enabled = true
embedding_model = "local-hash-v1"
embed_intra_threads = 1
storefront_candidates = true
storefront_seed_limit = 8
storefront_max_remote_calls = 32
# exclude_graphicaudio_series_sets = true  # opt-in; default keeps Magento series sets
openlibrary_enabled = true
# openlibrary_contact_email = "you@example.com"
openlibrary_max_requests_per_run = 25
listen_sync_interval_minutes = 60  # 0 disables daemon listening sync
recommend_limit = 20
```

Listening sync fans out through `IntegrationRegistry` (every integration with
`supports_listening_sync`). Daemon honors `listen_sync_interval_minutes`; CLI
`discover sync-listening` and `POST /api/discover/sync-listening` do the same.

Shelf visibility is **per-user** (SQLite `user_preferences`, not TOML). Use
Discover → settings in the GUI, or `GET` / `PATCH /api/preferences` with
`{ "disabled_shelves": ["chirp_deals", "genre"] }`. Empty list = all shelves on.
CLI `discover recommend` applies the operator prefs row.

## Surfaces

| Surface | Commands / routes |
| --- | --- |
| CLI | `bookclerk discover recommend` (prints shelves), `embed`, `sync-listening`, `request …` |
| Daemon | `GET /api/discover/recommendations` → `{ shelves, shelf_kinds }`, `POST /api/discover/purchase-hints` (live multi-store pricing), `CRUD /api/discover/requests`, `GET`/`PATCH /api/preferences` |
| GUI | Discover page — Netflix-style shelves; wishlist from any card into the request queue; cards fetch live prices and show the lowest-priced storefront plus links to other catalog matches |

### Shelf taxonomy

| Shelf | Kind id | Signal |
| --- | --- | --- |
| Finish these series | `finish_series` | Incomplete series + next index |
| Pick up where you left off | `keep_listening` | Active / multi-book listening in a series |
| More from {Author} | `author` | Liked-author overlap (top authors) |
| If you like {Author} | `because` | Related/similar titles **not** by that author |
| Narrated by {Narrator} | `narrator` | Liked-narrator overlap |
| Because you like {Genre} | `genre` | Category/subject overlap from finished titles |
| From {Store} | `from_store` (`from_audible`, …) | Candidates from storefronts already in the library |
| Chirp deals right now | `chirp_deals` | Chirp top + free deals |
| Similar to books you finish | `similar_taste` | Embedding similarity |
| Your requests | `requests` | Open title request queue |
| Top picks for you | `top_picks` | Fallback when every other shelf is empty |

All kinds are **offered by default**. Each user hides shelves via
`disabled_shelves` in `/api/preferences` (Discover settings in the GUI). The
recommendations endpoint applies the caller's prefs server-side; the API still
returns `shelf_kinds` so the prefs UI knows what can be toggled.

Kind matching: `author` hides every `author:…` row; `from_store` hides all
`from_*` rows; exact ids like `finish_series` or `from_chirp` also work.

## Store links and pricing

Recommendations are consolidated by **ISBN** (normalized), then **ASIN**, then
soft title+author matching so the same book from multiple storefronts appears
as **one card** with `store_editions` for each catalog match.

The feed seeds purchase URLs for every known edition (no live prices — those
change). At **view time** the GUI calls `POST /api/discover/purchase-hints`
with the title identity plus `store_editions`. The daemon:

1. Prices the known editions and expands other catalog matches when needed
2. Returns `{ hints, best }` sorted by ascending `price_cents` (`0` = free)

The Discover card highlights **`best`** (lowest known price) and lists the
other catalog matches as secondary links.

## Non-goals (this iteration)

- Chirp personalized endpoints that require a logged-in session
  (`currentUserRelatedAudiobooks`, wishlist, …)
- Bulk Open Library harvest (use dumps)
- Paid WorldCat / Goodreads / Hardcover providers
- A long-running vector DB sidecar
- Audible Spatial/Atmos (L1) merchandising
