# Operations

## Daemon (`bookclerkd`)

```bash
export BOOKCLERK_FILES_DIR=/var/lib/bookclerk
bookclerkd
```

Reads `config.toml` from the files dir (or `BOOKCLERK_CONFIG`). Schedules:

- library scan every `library.scan_interval_minutes`
- auto-acquire when `library.auto_acquire = true` (keep **false** until ready)

HTTP control plane (default `127.0.0.1:8787`):

| Endpoint | Auth | Purpose |
| --- | --- | --- |
| `GET /health` | no | Liveness |
| `POST /api/auth/login` | no | Operator token → session cookie |
| `GET /api/status` (also `/status`) | yes | Status snapshot |
| `POST /api/library/scan` (also `/scan`) | yes | Queue scan (`Content-Type: application/json`) |
| `POST /api/library/acquire` (also `/acquire`) | yes | Queue acquire |
| `GET /api/library/books` | yes | Book rows for the GUI |
| `GET /api/jobs` (also `/jobs`) | yes | Job list |
| `/` static UI | no | Built React SPA when `ui/dist` is present |

Override listen with `BOOKCLERK_DAEMON_LISTEN` or `daemon.listen`. Operator auth
defaults **on** (`[daemon.auth]`); token at `$BOOKCLERK_FILES_DIR/operator.token`
or `BOOKCLERK_OPERATOR_TOKEN`. Do not expose publicly without TLS (reverse
proxy) and a protected token. Details: [gui.md](gui.md).

Talk to a running daemon from the CLI (sends Bearer when auth is enabled):

```bash
bookclerk daemon health
bookclerk daemon status
bookclerk daemon jobs
bookclerk daemon scan [--account <id>]
bookclerk daemon acquire [--asin <id>] [--account <id>]
```

## systemd

Sample units:

- User session (tray-friendly): [`packaging/systemd/bookclerkd.user.service`](../packaging/systemd/bookclerkd.user.service)
- System-wide: [`packaging/systemd/bookclerkd.service`](../packaging/systemd/bookclerkd.service)

```bash
# Recommended (user session + tray): helper installs setuid-root bookclerkd,
# bookclerk system user, ~/Audiobooks ACLs, and the user systemd unit.
cargo build --release -p bookclerkd
./packaging/scripts/install-linux-user.sh ./target/release/bookclerkd

# Or system-wide:
sudo useradd --system --home /var/lib/bookclerk --shell /usr/sbin/nologin bookclerk
sudo mkdir -p /var/lib/bookclerk && sudo chown -R bookclerk:bookclerk /var/lib/bookclerk
sudo systemctl enable --now bookclerkd
# Set BOOKCLERK_OUTPUT_OWNER=<login> and ReadWritePaths for ~/Audiobooks.
```

Highlights:

- Process identity is **`bookclerk`** after privilege drop (isolation from the
  interactive user’s credentials and home).
- The **user unit** is owned by the installing user so the tray/session can
  attach; it sets `BOOKCLERK_OUTPUT_OWNER=%u`. The install script places a
  **setuid-root** `bookclerkd` that drops to `bookclerk` (real uid stays the
  installer for owner capture).
- Default media root is `@user/Audiobooks` → `~/Audiobooks`. On Linux, privilege
  drop retains **`CAP_CHOWN`** so acquired files can be chown’d to the owner;
  `setfacl` lets `bookclerk` write the directory.
- `ProtectHome=read-only` (not `true`) so explicit home `ReadWritePaths` work.
- Prefer `BOOKCLERK_AUTH_PASSWORD` (or `[auth].password`) — not under the files dir.

### Service identity (all platforms)

`bookclerkd` enforces a dedicated OS account via `[daemon.identity]`:

```toml
[daemon.identity]
service_user = "bookclerk"
service_group = "bookclerk"
drop_privileges = true          # root / setuid-root → setuid/setgid to service_user
allow_interactive_user = false  # refuse login-user runs against system data dirs
```

Before drop, Bookclerk captures the installing user into `BOOKCLERK_OUTPUT_OWNER`
when unset (`SUDO_USER` / real uid from a setuid helper / current interactive user).

| Situation | Behaviour |
| --- | --- |
| Started as `bookclerk` | OK |
| Started as root with `drop_privileges` | Drops to `bookclerk` before opening secrets; **fail-closed** if drop fails |
| Started as your login user with `/var/lib/bookclerk` | **Refused** |
| Started under `/tmp` / `$HOME` / `BookclerkFiles` (dev) | Allowed with a warning (non-root only) |
| Override | `BOOKCLERK_ALLOW_USER_RUN=1` or `allow_interactive_user=true` |

macOS LaunchDaemon: [`packaging/launchd/com.bookclerk.daemon.plist`](../packaging/launchd/com.bookclerk.daemon.plist).  
Windows service account notes: [`packaging/windows/README.md`](../packaging/windows/README.md).

## Docker

Dockerfile: [`packaging/docker/Dockerfile`](../packaging/docker/Dockerfile)
(repo-root convenience symlink: `Dockerfile`).

```bash
docker build -f packaging/docker/Dockerfile -t bookclerkd .
docker run --rm \
  -v bookclerk-config:/config \
  -v bookclerk-data:/data \
  -e BOOKCLERK_AUTH_PASSWORD='your-strong-passphrase' \
  bookclerkd
```

| Path / env | Role |
| --- | --- |
| `/config` | `BOOKCLERK_FILES_DIR` (`config.toml`, `library.db` incl. `encrypted_secrets`) |
| `/data` | Default books root (`BOOKCLERK_OUTPUT_LOCAL_ROOT=/data/Audiobooks`) |
| `BOOKCLERK_DAEMON_LISTEN` | Default loopback inside the container |

To publish the UI / API (keep operator auth enabled; terminate TLS at a proxy):

```bash
docker run … -e BOOKCLERK_DAEMON_LISTEN=0.0.0.0:8787 \
  -e BOOKCLERK_OPERATOR_TOKEN_FILE=/secrets/operator.token \
  -p 8787:8787 bookclerkd
```

Copy `/etc/bookclerk/config.example.toml` from the image as a starting config.

## Logging

- stderr + OS facility (journald / macOS `os_log` / Windows Event Log)
- `BOOKCLERK_LOG` → `RUST_LOG` → default `bookclerk=info,warn`
- CLI: `-v` / `-vv`
- Secrets are redacted; diagnostics uploads abort if a registered secret remains

Opt-in crash/error reporting: [diagnostics.md](diagnostics.md).

## Safe live testing

When exercising real store credentials:

1. Keep `library.auto_acquire = false`.
2. After login, `bookclerk auth set-scan <account> --scan false`.
3. Acquire **one** title (`--asin` / `--isbn` / UUID), not the whole library.
4. Prefer CLI over `POST /scan` / `/acquire` so nothing bulk-queues.
5. One-shot sync without re-enabling scan: `library scan --account <id>`.
