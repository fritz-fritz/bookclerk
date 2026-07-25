# Dynamic plugins

Libation loads **third-party plugins** declared in `config.toml` and talks to
them over newline-delimited [JSON-RPC 2.0](https://www.jsonrpc.org/specification)
on stdio. First-party sources/integrations stay in-process; external plugins are
separate executables that any language can implement.

## Why subprocesses?

Content sources and integrations need their own async runtimes, HTTPS clients,
and OAuth flows. Loading foreign `cdylib`s into the host process is fragile
across Rust/Tokio versions. A child process gives crash isolation, independent
releases, and a stable wire protocol.

## Declaring a plugin in `config.toml`

A `[sources.<id>]` or `[integrations.<id>]` table becomes an external plugin
when it sets **`command`** (path to an executable). Kind is inferred from the
section. Optional `args` is an array of extra argv. All other keys are opaque
knobs forwarded on handshake (except host-owned `command` / `args`).

```toml
[integrations.echo]
enabled = true
command = "plugins/echo/libation-plugin-echo-integration"
# args = ["--verbose"]
# greeting = "hi"          # example opaque knob

[sources.my_store]
enabled = true
command = "/opt/libation-plugins/my-store"
# … opaque knobs …
```

- Relative `command` paths resolve against `$LIBATION_FILES_DIR`.
- Bare names (no `/`) use the process `PATH` at spawn time.
- **External integrations default to disabled** unless `enabled = true`.
- Sources follow the usual `[sources.<id>]` rules (missing table → enabled).

There is no separate `plugin.toml`. Identity, capabilities, and brand come from
the plugin’s `handshake` response (the config key is the plugin id).

## Protocol (api_version = 1)

Host → plugin: one JSON object per line on stdin.  
Plugin → host: one JSON-RPC response per line on stdout.  
Stderr is free for logging.

### Common

| Method | Purpose |
| --- | --- |
| `handshake` | Negotiate version, id, kind, capabilities, brand |
| `health` | Connectivity / config check |
| `diagnose` | Human-readable CLI probe lines |

Handshake params include `{ "api_version": 1, "config": {…} }` (the plugin’s
TOML table as JSON, without `command` / `args`).

### Integration capabilities

Advertise in `handshake.capabilities`: `start`, `on_event`, `health`,
`diagnose`, `scan_library`, `authenticate_user`.

| Method | Notes |
| --- | --- |
| `start` | Background watchers |
| `on_event` | `{ "event": "book_liberated"\|"external_user_observed", "payload": … }` |
| `scan_library` | `{ "force": bool }` |
| `authenticate_user` | `{ "username", "password" }` → `ExternalUser` |

### Source capabilities

| Method | Notes |
| --- | --- |
| `login` / `list_accounts` | Auth under `files_dir` |
| `scan` | Receives `library_db` path; plugin opens SQLite |
| `fetch_title` | Write media under `cache_dir`; return `plain` paths |

Encrypted/DRM fetch is not in the v1 external protocol yet (first-party only).

### Output plugins

`kind = "output"` may appear in handshake later; config discovery for outputs is
not wired yet.

## Example

Build and point config at the binary:

```bash
cargo build -p libation-plugin-echo-integration
mkdir -p "$LIBATION_FILES_DIR/plugins/echo"
cp target/debug/libation-plugin-echo-integration \
   "$LIBATION_FILES_DIR/plugins/echo/"
```

```toml
# config.toml
[integrations.echo]
enabled = true
command = "plugins/echo/libation-plugin-echo-integration"
```

```bash
libation plugins list
libation integrations status
# echo enabled=true ok=true echo plugin ready
```

## Distribution

Ship a binary for the target OS/arch. Users place it anywhere readable (often
under `$LIBATION_FILES_DIR/plugins/`) and set `command` + `enabled` in
`config.toml`. No rebuild of Libation is required when the protocol version
matches.
