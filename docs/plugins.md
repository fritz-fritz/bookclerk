# Dynamic plugins

Libation discovers **third-party plugins** at runtime and talks to them over
newline-delimited [JSON-RPC 2.0](https://www.jsonrpc.org/specification) on
stdio. First-party sources/integrations stay in-process; external plugins are
separate executables that any language can implement.

## Why subprocesses?

Content sources and integrations need their own async runtimes, HTTPS clients,
and OAuth flows. Loading foreign `cdylib`s into the host process is fragile
across Rust/Tokio versions. A child process gives crash isolation, independent
releases, and a stable wire protocol.

## Layout

```text
$LIBATION_FILES_DIR/plugins/
  echo/
    plugin.toml
    libation-plugin-echo-integration   # executable
```

Additional roots: `LIBATION_PLUGIN_DIRS` (OS path list).

### `plugin.toml`

```toml
api_version = 1
id = "echo"
name = "Echo Integration"
kind = "integration"          # source | integration | output
command = "./libation-plugin-echo-integration"
# args = ["--verbose"]
```

`command` may be absolute or relative to the manifest directory.

## Enabling in config

Plugin ids must match a config table. **External integrations default to
disabled**; sources follow the usual `[sources.<id>]` rules (missing → enabled).

```toml
[integrations.echo]
enabled = true

[sources.my_store]
enabled = true
# … opaque knobs passed to the plugin on handshake …
```

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
TOML table as JSON).

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

`kind = "output"` is discovered and logged; loading is not implemented yet.

## Example

Build and install the echo integration:

```bash
cargo build -p libation-plugin-echo-integration
mkdir -p "$LIBATION_FILES_DIR/plugins/echo"
cp target/debug/libation-plugin-echo-integration \
   "$LIBATION_FILES_DIR/plugins/echo/"
cat > "$LIBATION_FILES_DIR/plugins/echo/plugin.toml" <<'EOF'
api_version = 1
id = "echo"
kind = "integration"
command = "./libation-plugin-echo-integration"
EOF
```

```toml
# config.toml
[integrations.echo]
enabled = true
```

```bash
libation integrations status
# echo enabled=true ok=true echo plugin ready
```

## Distribution

Ship a directory (or archive) containing `plugin.toml` + binary for the target
OS/arch. Users unpack under `plugins/` and set `enabled = true`. No rebuild of
Libation is required when the protocol version matches.
