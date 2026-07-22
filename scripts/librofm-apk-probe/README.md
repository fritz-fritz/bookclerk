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
- **Request contract**: `@Query` / `@Path` / `@Body` parameter names on each
  Retrofit method (e.g. `download-manifest` → `isbn`, `client_version`, `format`)
- **Response / body shapes**: Gson `@SerializedName` + unannotated field names
  on request/response DTOs, including nested children (e.g. `tracks[]` →
  `length_msec`, `chapter_title`)
- OAuth password-grant path (`/oauth/token`) plus `AuthPasswordRequest` /
  `AuthResponse` field names

Then it diffs:

1. Tracked path/header/version constants against
   `crates/libation-libro/src/client.rs`
2. Tracked request/response shapes (incl. nested DTO fields) against
   `scripts/librofm-apk-probe/expected_shapes.json`

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

### Limits of static APK analysis

- Declared Gson/Retrofit contracts are **not** a live wire dump. Optional fields,
  server-only keys, and polymorphic payloads may differ at runtime.
- We **auto-update path/version/UA constants** on drift (after smoke). We do
  **not** auto-rewrite Rust `serde` structs — field renames need a reviewed PR.
  Schema drifts show up in the probe report / issue body so a human (or agent)
  can patch `client.rs` and refresh `expected_shapes.json`.

### Live API validation (GitHub secrets)

When repository secrets are set, CI runs `live_smoke.py` against both the
current client constants and the APK-extracted paths, and compares live JSON
keys (top-level + nested samples) to the APK-declared response fields:

- `TEST_LIBRO_EMAIL` or `TEST_LIBRO_USERNAME` or `TEST_LIBRO_USER`
- `TEST_LIBRO_PASSWORD`
- optional `TEST_LIBRO_ISBN` (otherwise first library ISBN; metadata only —
  no audio download)

If constants drift but smoke fails, CI opens an issue (no auto-PR). Without
secrets, APK schema extraction still runs and surfaces declared-field drifts;
live confirmation is what proves the wire format.

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
export TEST_LIBRO_EMAIL='you@example.com'   # or TEST_LIBRO_USER / TEST_LIBRO_USERNAME
export TEST_LIBRO_PASSWORD='…'   # never pass it on argv
# optional: export TEST_LIBRO_ISBN='978…'  # one book only
python3 scripts/librofm-apk-probe/live_smoke.py --profiles current,apk
```

Reports land in `artifacts/librofm-apk-probe/` (`report.md`, `report.json`,
`apk_shapes.json`, `endpoints.txt`, optional `live_smoke.json`). Exit `1`
means tracked constants/schema drifted (extract) or a live call/schema check
failed (smoke); exit `2` is a hard failure / missing credentials.

## CI

`.github/workflows/librofm-apk-probe.yml` runs weekly and on `workflow_dispatch`.

1. Extract APK API surface (paths + request/response shapes) and upload artifacts
2. Live-smoke **current** and **APK-extracted** profiles when repository secrets
   are set (see above)
3. On drift (schedule/manual): if smoke passed or secrets were missing, open a
   PR on `chore/librofm-apk-api-sync` that updates `client.rs` constants. If
   smoke fails, open an issue instead (no PR). Schema-only drifts are reported
   in the artifact/`expected_shapes.json` diff for manual Rust updates.

Wiremock tests bind to the path constants, so constant bumps do not require
hand-editing fixtures.
