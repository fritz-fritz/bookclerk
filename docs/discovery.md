# Discovery, recommendations, and request queue

Bookclerk’s discovery stack turns **local taste** (owned library + listening)
into **unowned storefront candidates**, then ranks those with embeddings and
heuristics. Operator-facing surfaces ship first; the schema is ready for
Connect-portal identities later.

## Goals

1. **Discover unowned titles** from storefront catalogs (Libro related books,
   Audible public search / series ASIN, Chirp GraphQL related/series/author,
   GraphicAudio Magento series + related), not only from the owned library.
2. **Evaluate** those candidates against local ownership, listening, ratings,
   and embeddings.
3. **Queue requests** (“please buy this”) with optional preferred storefront.
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
                            author / catalog search
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
them.

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
   stronger still when multiple books in that series have listening activity
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
listen_sync_interval_minutes = 60
recommend_limit = 20
```

## Surfaces

| Surface | Commands / routes |
| --- | --- |
| CLI | `bookclerk discover recommend` (prints shelves), `embed`, `sync-listening`, `request …` |
| Daemon | `GET /api/discover/recommendations` → `{ shelves: [...] }`, `CRUD /api/discover/requests` |
| GUI | Discover page — Netflix-style shelves (series, listening, authors, “if you like…”, narrators, similar, requests) |

### Shelf taxonomy (v1)

| Shelf | Signal |
| --- | --- |
| Finish these series | Incomplete series + next index |
| Pick up where you left off | Active / multi-book listening in a series |
| More from {Author} | Liked-author overlap (top authors) |
| If you like {Author} | Related/similar titles **not** by that author |
| Narrated by {Narrator} | Liked-narrator overlap |
| Similar to books you finish | Embedding similarity |
| Your requests | Open title request queue |

## Non-goals (this iteration)

- Chirp personalized endpoints that require a logged-in session
  (`currentUserRelatedAudiobooks`, wishlist, …)
- Bulk Open Library harvest (use dumps)
- Paid WorldCat / Goodreads / Hardcover providers
- A long-running vector DB sidecar
- Audible Spatial/Atmos (L1) merchandising
