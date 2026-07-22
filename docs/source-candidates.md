# Audiobook source candidates (beyond Audible / Libro.fm)

Research notes on stores that could plug into `libation-source`'s
`ContentSource` trait. Criteria:

1. Public API **or** reverse-engineerable Android APK (Libro.fm pattern)
2. DRM-free audio **or** community-documented decrypt path
3. No DRM mention → treat as **unknown** until investigated

APKs were downloaded via `apkeep` (APKPure) and decompiled with `jadx`
(2026-07-22). Package IDs and key endpoints below are from those APKs plus
community tooling ([audiobook-dl](https://github.com/jo1gi/audiobook-dl),
[kobodl](https://github.com/subdavis/kobo-book-downloader), etc.).

## Fit with current architecture

| Concern | Notes |
| --- | --- |
| Trait surface | `login` / `list_accounts` / `scan` / `fetch_title` → `SourceFetch::Plain` or `Encrypted` |
| Plain path | Matches Libro.fm today (no `libation-decrypt`) |
| Encrypted path | Today: Adrm + Widevine only; new DRM kinds need decrypt work |
| Auth files | Per-account tokens under `Accounts/` |

---

## Priority ranking

| Rank | Source | Access | DRM | Integration fit |
| ---: | --- | --- | --- | --- |
| 1 | **GraphicAudio** | Clear Retrofit API in APK | DRM-free (official ZIP / app Lo+Hi URLs) | Excellent — Libro-like |
| 2 | **Kobo Audiobooks** | Documented `storeapi.kobo.com` (kobodl) | Audiobook spine usually plain; ebooks use KDRM | Strong — Audible-like auth |
| 3 | **Chirp** | GraphQL web + Android Mockingjay APIs | URL AES on web; local download encryptor in app; files play as MP3 | Strong with caveats |
| 4 | **Storytel / Mofibo** | Android API (`api.storytel.net`) | Serves `audio/mpeg` after auth (community) | Strong; rate-limit risk |
| 5 | **Audiobooks.com** | `api.audiobooks.com/api/v2/` in APK | Stream page yields MP3 (community) | Good; Storytel-owned |
| 6 | **Downpour** | `app.downpour.com` + web downloads | Mixed: mostly DRM-free, some DRM titles | Medium — filter `isDrm` |
| 7 | **LibriVox** | Public catalog API | Public domain / free | Easy but niche |
| 8 | **Podimo** | GraphQL gateway | Unknown (CDN mp3/m3u8 present) | Medium / investigate |
| — | Google Play Books | Partial official export | Per-title export flag | Weak for library sync |
| — | Spotify / Everand | — | Heavy / app-bound DRM | Poor fit |

---

## 1. GraphicAudio — best next source

**Why:** Same integration shape as Libro.fm: password login → library list →
plain download URLs. Official store also sells DRM-free MP3/M4B/FLAC ZIPs.

**APK:** `com.efsharp.graphicaudio`

**Base URL:** `https://www.graphicaudio.net/access/`

| Method | Path | Role |
| --- | --- | --- |
| `POST` | `activation/login` | Form: `username`, `password`, `client_id` |
| `POST` | `activation/thirdparty` | Token activate |
| `POST` | `activation/remove` | Forget device |
| `GET` | `api/products` | Library (`Authorization` header) |
| `GET` | `api/links?product=` | Download URLs |
| `GET`/`PUT` | `api/bookmark` | Progress |

`DownloadInfoResponse` fields: `Lo` / `Hi` → low/high quality stream/download
URLs (m4a in-app). No Widevine/MediaDrm usage in app code beyond ExoPlayer
boilerplate.

**Libation mapping:** New `SourceKind::GraphicAudio`, `SourceFetch::Plain`,
auth file e.g. `Accounts/*.ga.auth`. Optional: also scrape Magento
“My Downloadable Products” for ZIP MP3/M4B/FLAC purchases.

**Risks:** Niche catalog (dramatized productions). Device activation may
cap concurrent clients (same pattern as many mobile apps).

---

## 2. Kobo Audiobooks

**Why:** Mature community client already emulates device auth and downloads
audiobook spines as numbered audio files. Closest “Audible-sized” commercial
catalog among DRM-tractable options.

**APK:** `com.kobobooks.android` (large; strings confirm audiobook player +
ExoPlayer DRM stack for *ebooks* / streaming)

**API (from kobodl, still current pattern):**

- Device auth: `https://storeapi.kobo.com/v1/auth/device`
- Refresh: `…/v1/auth/refresh`
- Init resources: `…/v1/initialization`
- Library sync: tokenized `library_sync`
- Audiobook download URL → JSON `Spine[]` with `Url` + `FileExtension`

**DRM:** Ebook path uses KDRM / AdobeDrm. Audiobook download path in kobodl
skips content-key decrypt when `DrmType` is not KDRM/AdobeDrm and writes
plain part files. Treat as **plain for typical audiobooks**; verify live
before committing.

**Libation mapping:** Device-activation login (browser code, like Audible
QR/OAuth UX), `SourceFetch::Plain` for spine parts (ZIP/M4B packaging in
liberate). Do **not** need KDRM remover unless we also want ebooks.

**Risks:** Activation UX; API churn; regional audiobook availability.

---

## 3. Chirp (Pubmark)

**Why:** Deal-focused owned library; strong community downloaders; APK exposes
dedicated API hosts.

**APK:** `com.chirpbooks.chirp`

| Constant | Value |
| --- | --- |
| `MOCKINGJAY_API_ROOT` | `https://api.chirpbooks.com/api` |
| `MOCKINGJAY_LISTEN_API_ROOT` | `https://listen-api.chirpbooks.com/api` |
| Web | `https://www.chirpbooks.com` |

Android uses Apollo GraphQL (`com.mockingjay.*` queries/mutations:
`AndroidCurrentUserAudiobooksQuery`, sign-in, archive, position, …) plus
“Kingfisher” playback/download. Web/community path
([audiobook-dl](https://github.com/jo1gi/audiobook-dl)):

1. Cookie session
2. GraphQL `fetchAudiobookTracks` / `fetchAudiobookTrackUrl`
3. AES-CBC decrypt of `webPlayerMediaUrl` (key from page `data-dk`, IV from
   padded `user_id`)
4. Resulting URL is playable **MP3**

App also has `KingfisherDownloadedTrackEncryptor` / ExoPlayer offline cache —
local-at-rest encryption for offline listening, separate from the web URL
obfuscation.

**Libation mapping:** Prefer documented web GraphQL + cookie/password login →
`Plain` MP3 chapters. APK GraphQL schema is a second source of truth (probe
script like Libro).

**Risks:** Cloudflare / cookie fragility; URL crypto may change; Findaway
distribution roots appear in cover CDN paths (monitor for future DRM shifts).

---

## 4. Storytel / Mofibo

**Why:** Large subscription catalog; community client already implements
login + MP3 asset fetch.

**APK:** `grit.storytel.app` (decompiled; Kotlin/`com.storytel.*`)

**Documented API (audiobook-dl):**

| Call | Role |
| --- | --- |
| `POST …/api/login.action` | Password AES-CBC with fixed key/IV, Android UA |
| `GET api.storytel.net/book-details/consumables/{id}` | Metadata |
| `GET api.storytel.net/assets/v2/consumables/{id}/abook` | 302 → MP3 |
| `GET api.storytel.net/playback-metadata/…` | Chapters |
| `POST api.storytel.net/libraries/bookshelf` | Library |

**DRM:** Download responses expected as `audio/mpeg`. Not classic Widevine
file DRM in the community path. Treat liberate as `Plain`, but watch for
CDN/format changes. Aggressive download rate-limits can invalidate sessions.

**Libation mapping:** Password login + JWT; scan bookshelf; `Plain` fetch.
Subscription semantics differ from purchase libraries (titles can leave the
catalog).

**Risks:** Cloudflare; rate limits; ToS / account flags; regional catalogs.

---

## 5. Audiobooks.com (Storytel USA)

**APK:** `com.audiobooks.androidapp`

**API base in APK:** `https://api.audiobooks.com/api/v2/`

Community web path scrapes `/book/stream/{id}` for an embedded `mp3:` URL
using session cookies (`ci_session`). App REST surface is a better long-term
target than HTML scrape.

**DRM:** Unknown officially; community obtains plain MP3 streams. Investigate
app `/api/v2/` download endpoints before locking the design.

**Fit:** Good if overlapping Storytel work; US catalog / credits model.

---

## 6. Downpour (Blackstone)

**APK:** `com.blackstonehybrid`

**Hosts:** `https://app.downpour.com/` (app API), `cdn.blackstonepublishing.com`,
storefront `downpour.com` (Shopify + UCP commerce — not library download).

Entitlement model includes `isDrm()`. FAQ: most titles DRM-free MP3; some
publisher-required DRM playable only in-app. App uses Readium audiobook
manifests.

**Libation mapping:** Prefer DRM-free web account downloads when available;
app API for library sync, skip/error when `isDrm == true`. Heavy Kotlin
obfuscation → slower RE than GraphicAudio.

**Risks:** Mixed DRM; obfuscated client; need live classification of titles.

---

## 7. LibriVox

**Public API:** `https://librivox.org/api/feed/audiobooks` (+ audiotracks,
authors). No auth. DRM-free public domain.

**Fit:** Trivial `Plain` source; valuable for tests / CI without credentials,
not a commercial library manager feature.

---

## 8. Podimo

**APK:** `com.podimo`

**Hosts:** `https://graphql.pdm-gateway.com/graphql`, CDN
`cdn.podimo.com/audios/…mp3|m3u8`.

**DRM:** No clear documentation. Presence of bare MP3 CDN URLs is promising
but unproven for audiobook entitlements. Community support in audiobook-dl
via password login.

**Fit:** Worth a follow-up GraphQL schema dump; not first priority.

---

## Lower priority / poor fit

| Source | Verdict |
| --- | --- |
| **Google Play Books** | Some titles offer official M4A export; no good library/download API for automation; eBook DRM tooling ≠ audiobooks |
| **Spotify Audiobooks** | App-bound DRM; no credible personal-library liberate path |
| **Everand / Scribd** | Streaming subscription; cookie scrapers exist; DRM/ToS hostile to Libation model |
| **Nextory** | audiobook-dl login support; APK not fetched this pass; treat DRM as unknown |
| **OverDrive / Libby / cloudLibrary** | Library lending, not ownership; different product semantics |

---

## Recommended implementation order

1. **GraphicAudio** — smallest Libro-shaped spike (`ContentSource` + Plain
   liberate + APK probe script).
2. **Kobo audiobooks** — reuse kobodl protocol knowledge; device-activation
   auth UX.
3. **Chirp** — GraphQL + URL decrypt; optional APK Mockingjay probe.
4. **Storytel / Audiobooks.com** — shared Storytel-family learnings; handle
   subscription semantics explicitly in UX/docs.
5. **Downpour** — only after `isDrm` filtering is validated live.

LibriVox can land anytime as a zero-auth sandbox source.

## Open questions before coding

- Confirm GraphicAudio `api/links` returns durable HTTPS media (not short-lived
  signed URLs only) and device-activation limits.
- Live-check one Kobo audiobook spine for empty/`None` DRM fields.
- Decide whether subscription sources (Storytel, Podimo) belong in Libation
  (owned-library product) or stay out of scope.
- Chirp: cookie vs password auth for headless daemon use.
