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

## Install modes (Linux & macOS)

Bookclerk uses two privilege models. We **do not** ship a setuid-root binary
or keep real uid 0 via `seteuid` — those patterns enlarge the blast radius of
any RCE in the daemon.

| Mode | When | Process identity | How `~/Audiobooks` stays yours |
| --- | --- | --- | --- |
| **Session** | Desktop / tray / `systemd --user` | Login user | Natural ownership (you create the files) |
| **Service** | Headless / multi-user / hardened | `bookclerk` from process start | Linux: systemd **ambient `CAP_CHOWN`**; all platforms: **ACL grant** to `BOOKCLERK_OUTPUT_OWNER` after write |

### Why not setuid-root or keep-root seteuid?

- **setuid-root helper** (former user-unit path): every exec starts with euid 0.
  A bug before drop is full root. File capabilities on the daemon binary are
  similar — *any* local exec of that path inherits the cap
  ([capability hardening guidance](https://www.systemshardening.com/articles/linux/linux-capability-hardening/)).
- **`seteuid(bookclerk)` with ruid 0** (former macOS path): a compromised
  process calls `seteuid(0)` and is root again. macOS has no `CAP_CHOWN` to
  retain across a real `setuid`.
- **`systemd --user` + AmbientCapabilities**: user managers clear ambient caps;
  this is not a reliable way to run as `bookclerk` from a user unit
  ([systemd#33167](https://github.com/systemd/systemd/issues/33167)).

So: tray-friendly installs run **as you**; hardened installs are **system**
units/LaunchDaemons that never start as root.

### Session mode (recommended for tray)

```bash
cargo build --release -p bookclerkd
./packaging/scripts/install-linux-user.sh ./target/release/bookclerkd
```

- Unit: [`bookclerkd.user.service`](../packaging/systemd/bookclerkd.user.service)
- Binary mode `755` (not setuid); `NoNewPrivileges=true`
- Seeded config: `allow_interactive_user = true`
- Media: `@user/Audiobooks` → `~/Audiobooks` owned by you

### Service mode (hardened)

```bash
sudo useradd --system --home /var/lib/bookclerk --shell /usr/sbin/nologin bookclerk
sudo mkdir -p /var/lib/bookclerk && sudo chown -R bookclerk:bookclerk /var/lib/bookclerk
sudo install -o root -g root -m 755 ./target/release/bookclerkd /usr/local/bin/bookclerkd
# Edit packaging/systemd/bookclerkd.service: BOOKCLERK_OUTPUT_OWNER + ReadWritePaths
sudo cp packaging/systemd/bookclerkd.service /etc/systemd/system/
sudo systemctl enable --now bookclerkd
```

Unit highlights ([`bookclerkd.service`](../packaging/systemd/bookclerkd.service)):

```ini
User=bookclerk
AmbientCapabilities=CAP_CHOWN
CapabilityBoundingSet=CAP_CHOWN
NoNewPrivileges=true
```

systemd grants `CAP_CHOWN` while switching to `User=` (no root stage in our
code). Bounding set prevents any other capability. After write we also
**ACL-grant** the owner (`setfacl` / macOS `chmod +a`) so media stays usable
even if chown is unavailable.

Grant `bookclerk` write on the media tree once:

```bash
setfacl -m u:bookclerk:rwx -d -m u:bookclerk:rwx /home/alice/Audiobooks
```

### macOS

LaunchDaemon: [`com.bookclerk.daemon.plist`](../packaging/launchd/com.bookclerk.daemon.plist)
— `UserName=bookclerk` (not root). No `seteuid` keep-root. Ownership transfer
uses ACL grants to `BOOKCLERK_OUTPUT_OWNER`; a future SMAppService XPC helper
could narrow chown further for app-bundled installs, but is not required for
CLI/daemon packages.

For a login-session Mac install, run `bookclerkd` as your user (same as Linux
session mode) instead of the LaunchDaemon.

Windows: [`packaging/windows/README.md`](../packaging/windows/README.md).

### Service identity knobs

```toml
[daemon.identity]
service_user = "bookclerk"
service_group = "bookclerk"
drop_privileges = true          # if started as root → full setuid to service_user
allow_interactive_user = false  # session install sets true
```

| Situation | Behaviour |
| --- | --- |
| Started as `bookclerk` (service unit) | OK |
| Started as root with `drop_privileges` | Full `setuid`/`setgid` to `bookclerk` (Linux may keep only `CAP_CHOWN`); **fail-closed** |
| Session install (`allow_interactive_user=true`) | Login user OK |
| Login user + `/var/lib/bookclerk` without allow | **Refused** |
| `/tmp` / `BookclerkFiles` scratch (dev) | Allowed with a warning (non-root) |
| Override | `BOOKCLERK_ALLOW_USER_RUN=1` |

Env **`BOOKCLERK_OUTPUT_OWNER`** overrides `output.local.owner_user`. Prefer
`BOOKCLERK_AUTH_PASSWORD` for wrapping `master.key`.

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
