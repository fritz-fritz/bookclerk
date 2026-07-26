# GUI (web + desktop)

Bookclerk ships a shared React UI for library management. The UI talks to the
Rust HTTP API on `bookclerkd` only (TypeScript never hosts the API).

## Status

| Surface | Support |
| --- | --- |
| Web UI via `bookclerkd` | Supported on all platforms |
| Tauri desktop (`desktop/bookclerk-desktop`) | **Windows and macOS** (nested workspace) |
| Linux native window | Deferred until Tauri GTK4 — use web UI or tray+browser |

The desktop crate lives in a **nested Cargo workspace** under `desktop/` so
Tauri’s Linux GTK3 / WebKitGTK packages never enter the root `Cargo.lock`
(OSV gate). Path evaluation and the close-out of
[#44](https://github.com/fritz-fritz/bookclerk/pull/44):
[gui-desktop-path.md](gui-desktop-path.md).

## Operator auth

The GUI / `/api/*` routes use a separate **operator** credential from the
Connect portal:

| Item | Detail |
| --- | --- |
| Token file | `$BOOKCLERK_FILES_DIR/operator.token` (or `[daemon.auth].token_file`) |
| Env | `BOOKCLERK_OPERATOR_TOKEN` / `BOOKCLERK_OPERATOR_TOKEN_FILE` |
| Browser | `POST /api/auth/login` → HttpOnly cookie `bookclerk_operator_session` |
| CLI / automation | `Authorization: Bearer <token>` |
| Config | `[daemon.auth] enabled`, `token_file`, `session_ttl_hours` |

On first start with auth enabled, `bookclerkd` mints the token file (`0600`) and
prints its path to stderr. Auth is required by default; binding a non-loopback
address with `daemon.auth.enabled = false` is rejected at startup.

Prefer TLS termination at a reverse proxy when exposing the UI remotely.

## Web UI (served by `bookclerkd`)

```bash
cd ui && npm ci && npm run build
export BOOKCLERK_FILES_DIR=/tmp/BookclerkFiles
cargo run -p bookclerkd
# open http://127.0.0.1:8787/ and paste the operator token
```

Dev (Vite with API proxy):

```bash
# terminal 1
BOOKCLERK_FILES_DIR=/tmp/BookclerkFiles cargo run -p bookclerkd
# terminal 2
cd ui && npm run dev   # http://127.0.0.1:5173
```

Override the static dist directory with `BOOKCLERK_UI_DIST`.

### MVP screens

- Operator login
- Dense book rows (cover, title, authors/narrators, series, source, status)
- Search + status filter
- Scan / acquire pending / acquire one title
- Jobs + status strip

## Desktop (`bookclerk-desktop`) — Windows / macOS

Tauri 2 shell: loads the same UI, shows a tray icon (Show / Hide / Scan / Quit),
spawns `bookclerkd` when the configured listen address is unreachable, and
injects the operator token for auto-login via `invoke("operator_token")`
(optional `@tauri-apps/api` in the UI; ignored in the browser).

```bash
cd ui && npm ci && npm run build
cargo build -p bookclerkd
cargo build --manifest-path desktop/Cargo.toml -p bookclerk-desktop
```

CI builds this job on `macos-latest` and `windows-latest` only. Linux builds
panic in `build.rs` with a pointer to tray/web alternatives until upstream
Tauri ships GTK4 + WebKitGTK 6 without the advisory-pinned GTK3 graph.

**Do not** add `bookclerk-desktop` as a root workspace member, and **do not**
add OSV `IgnoredVulns` for gtk in the root lockfile.

## Brand assets

Production logo/mark/favicons live under [`assets/brand/`](../assets/brand/).
The UI copies web assets into `ui/public/`.
