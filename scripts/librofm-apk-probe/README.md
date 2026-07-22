# Libro.fm APK API probe

The Libro.fm integration in `crates/libation-libro` follows the unofficial
mobile API used by the Android app (`fm.libro.librofm`). Community clients
originally reverse-engineered those calls; this probe keeps that surface
honest by re-extracting it from the latest Play Store APK.

## What APK analysis can (and cannot) do

### Captured from the decompiled APK (jadx)

- `BuildConfig` / `AppModule` — base URL, `/api/vN/` prefix, app version
- `AuthInterceptor` — `X-LibroFm-*` and `Authorization` headers
- OkHttp embedded user-agent (`okhttp/x.y.z`)
- Retrofit `@GET` / `@POST` / … paths on `*Api.java` interfaces
- **Request contract**: `@Query` / `@Path` / `@Body` parameter names
- **Response / body shapes**: Gson `@SerializedName` + nested DTO fields
- OAuth password-grant path (`/oauth/token`) plus auth request/response fields

Then it diffs tracked path/header/version constants against
`crates/libation-libro/src/client.rs` and request/response shapes against
`scripts/librofm-apk-probe/expected_shapes.json`.

| Client constant | APK source |
| --- | --- |
| `DEFAULT_BASE_URL` | `AppModule.provideApiBaseUrl` / `BuildConfig.BASE_URL` |
| `OAUTH_TOKEN_PATH` | `LoginRepoImpl` → `{base}/oauth/token` |
| `LIBRARY_PATH` | `{api_prefix}library` |
| `DOWNLOAD_MANIFEST_PATH` | `{api_prefix}download-manifest` |
| `PACKAGED_M4B_PATH` | `{api_prefix}audiobooks/{isbn}/packaged_m4b` |
| `APP_VER` | APK `versionName` / `BuildConfig.VERSION_NAME` |
| `USER_AGENT_VALUE` | OkHttp `userAgent` |

### Limits of static APK analysis

Declared Gson/Retrofit contracts are **not** a live wire dump. We auto-update
path/version/UA constants after a successful **auth** smoke. We do **not**
auto-rewrite Rust `serde` structs.

## Live API validation (auth required for liberate path)

`libation-libro` uses oauth → library → packaged_m4b / download-manifest → CDN
bytes. Those calls need a library account. Catalog `explore/*` exists and is
unauthenticated, but the client does not use it today — public smoke is
informational only.

### Auth smoke (CI gate)

Requires repository secrets:

- `TEST_LIBRO_EMAIL` or `TEST_LIBRO_USERNAME` or `TEST_LIBRO_USER`
- `TEST_LIBRO_PASSWORD`
- optional `TEST_LIBRO_ISBN` — one **library** title (prefer a short book)
- optional `TEST_LIBRO_MAX_DOWNLOAD_BYTES` — default `104857600` (100 MiB)

Flow:

1. OAuth password grant
2. Library page 1 (+ schema check)
3. Packaged M4B meta (404 OK)
4. Download-manifest (+ schema check, including `tracks[].length_msec`)
5. Download **one** media asset (M4B URL preferred, else first manifest part)
6. Probe magic bytes: `ftyp` → m4b/mp4, `ID3`/MPEG sync → mp3, `PK` zip with
   audio entries → zip parts

If the object is larger than the max, CI probes the first 2 MiB and notes a
partial download (set ISBN to a shorter title for a full pull).

Constant auto-PRs run only when auth smoke **passes**. If secrets are missing
on drift, CI opens an issue instead of a PR.

### Public catalog (optional)

`live_smoke.py --profiles public` hits `explore/search`, suggest, genres, and
`explore/audiobook_details/{isbn}` without auth. Useful for catalog metadata
shapes; not a substitute for liberate-path checks.

## Local usage

```bash
# Download latest APK + decompile + diff (needs network + Java for jadx)
python3 scripts/librofm-apk-probe/extract_libro_api.py

# Auth smoke with media download + probe (needs credentials)
export TEST_LIBRO_EMAIL='you@example.com'
export TEST_LIBRO_PASSWORD='…'   # never on argv
# optional: export TEST_LIBRO_ISBN='978…'  # one library book
python3 scripts/librofm-apk-probe/live_smoke.py --profiles current,apk

# JSON-only (skip CDN bytes)
python3 scripts/librofm-apk-probe/live_smoke.py --profiles current --no-media-download

# Public catalog only
python3 scripts/librofm-apk-probe/live_smoke.py --profiles public
```

Reports land in `artifacts/librofm-apk-probe/` (`report.md`, `report.json`,
`apk_shapes.json`, `live_smoke.json`). Exit `1` = drift or smoke/media failure;
exit `2` = hard failure / missing auth credentials.

## CI

`.github/workflows/librofm-apk-probe.yml` runs weekly and on `workflow_dispatch`.

1. Extract APK API surface + upload artifacts
2. Optional public catalog smoke (informational)
3. Auth live-smoke + **media probe** when `TEST_LIBRO_*` secrets are set
4. On drift (schedule/manual): open a constants PR only if auth smoke passed;
   otherwise open an issue (missing secrets or smoke/media failure)

Wiremock tests bind to the path constants, so constant bumps do not require
hand-editing fixtures.
