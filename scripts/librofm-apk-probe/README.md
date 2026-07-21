# Libro.fm APK API probe

The Libro.fm integration in `crates/libation-libro` follows the unofficial
mobile API used by the Android app (`fm.libro.librofm`). Community clients
originally reverse-engineered those calls; this probe keeps that surface
honest by re-extracting it from the latest Play Store APK.

## What it extracts

From the decompiled APK (jadx):

- `BuildConfig` / `AppModule` — base URL, `/api/vN/` prefix, app version
- `AuthInterceptor` — `X-LibroFm-*` and `Authorization` headers
- OkHttp embedded user-agent (`okhttp/x.y.z`)
- Retrofit `@GET` / `@POST` / … paths on `*Api.java` interfaces
- OAuth password-grant path (`/oauth/token`)

Then it diffs tracked fields against
`crates/libation-libro/src/client.rs`:

| Client constant | APK source |
| --- | --- |
| `DEFAULT_BASE_URL` | `AppModule.provideApiBaseUrl` / `BuildConfig.BASE_URL` |
| `OAUTH_TOKEN_PATH` | `LoginRepoImpl` → `{base}/oauth/token` |
| `LIBRARY_PATH` | `{api_prefix}library` |
| `DOWNLOAD_MANIFEST_PATH` | `{api_prefix}download-manifest` |
| `PACKAGED_M4B_PATH` | `{api_prefix}audiobooks/{isbn}/packaged_m4b` |
| `APP_VER` | APK `versionName` / `BuildConfig.VERSION_NAME` |
| `USER_AGENT_VALUE` | OkHttp `userAgent` |

Secret-looking `BuildConfig` fields (e.g. `API_KEY`) are redacted to a
sha256 fingerprint in reports. Prod builds do not send `X-LibroFm-Api-Key`
(see `AuthInterceptor`).

## Local usage

```bash
# Download latest APK + decompile + diff (needs network + Java for jadx)
python3 scripts/librofm-apk-probe/extract_libro_api.py

# Reuse an already-downloaded package
python3 scripts/librofm-apk-probe/extract_libro_api.py --apk /path/to/fm.libro.librofm.xapk

# Keep scratch files for inspection
python3 scripts/librofm-apk-probe/extract_libro_api.py --workdir /tmp/libro-probe

# Apply suggested constant updates to client.rs
python3 scripts/librofm-apk-probe/apply_client_updates.py

# Live-smoke current constants + APK-extracted paths (needs credentials)
export TEST_LIBRO_EMAIL='you@example.com'
export TEST_LIBRO_PASSWORD='…'   # never pass on argv
# optional: export TEST_LIBRO_ISBN='978…'  # one book only
python3 scripts/librofm-apk-probe/live_smoke.py --profiles current,apk
```

Reports land in `artifacts/librofm-apk-probe/` (`report.md`, `report.json`,
`endpoints.txt`, optional `live_smoke.json`). Exit `1` means tracked constants
drifted (extract) or a live call failed (smoke); exit `2` is a hard failure /
missing credentials.

## CI

`.github/workflows/librofm-apk-probe.yml` runs weekly and on `workflow_dispatch`.

1. Extract APK API surface and upload artifacts
2. Live-smoke **current** and **APK-extracted** profiles when repository secrets
   are set:
   - `TEST_LIBRO_EMAIL` or `TEST_LIBRO_USERNAME`
   - `TEST_LIBRO_PASSWORD`
   - optional `TEST_LIBRO_ISBN` (otherwise first library ISBN; metadata only —
     no audio download)
3. On drift (schedule/manual): if smoke passed or secrets were missing, open a
   PR on `chore/librofm-apk-api-sync` that updates `client.rs`. If smoke fails,
   open an issue instead (no PR).

Wiremock tests bind to the path constants, so constant bumps do not require
hand-editing fixtures.
