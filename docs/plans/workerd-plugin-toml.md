# Plan: workerd isolates + greenfield `plugin.toml`

Status: **design proposal** (not implemented).  
Scope: redesign the installed plugin manifest so Bookclerk can load guests as
[Dynamic Workers](https://developers.cloudflare.com/dynamic-workers/) /
[`WorkerCode`](https://developers.cloudflare.com/dynamic-workers/api-reference/)
under a shared ABI, with full fidelity to workerd capabilities
(`compatibility_date`, flags, modules, egress, limits, host bindings).

This document is intentionally **greenfield**. It does not preserve the current
`command` / `args` / Landlock `[sandbox].network` shape except as a critique of
what to abandon.

## Context from prior plans

The cited agent id `bc-98e3cbf5-3c6a-4947-a3ef-149d9c11efb9` was not available
to this run. Closest related work:

| Source | What it settled |
| --- | --- |
| [Bookclerk plugin infrastructure plan](https://cursor.com/agents/bc-019fbb7d-facc-716d-8fba-2edb3ec334e2) (PR #74+) | Native OS subprocesses + `jsonrpc-stdio-v1`; **rejected** WASM / cloud sandboxes / in-process `cdylib` |
| [Decentralized WASM plugin plan](https://cursor.com/agents/bc-019fbb28-8578-7f8a-a454-33b32cd42fa6) | Hybrid: keep native JSON-RPC for DRM/SQLite/S3; add **Wasmtime WASI P2** later with JSON-over-WIT |

Neither plan designed `plugin.toml` against Cloudflare’s `WorkerCode` object.
That is the gap this proposal fills: if the runtime is workerd isolates, the
manifest must be a direct, honest projection of what the loader will pass to
`env.LOADER.get()` / `load()`, plus Bookclerk identity and capability *requests*.

## Critique of the current `plugin.toml`

Today (`crates/bookclerk-plugin-host/src/manifest.rs`):

```toml
api_version = 1
id = "echo"
kind = "integration"
command = "./bookclerk-plugin-echo-integration"
[sandbox]
network = "none"
```

Problems for a workerd world:

1. **`command` / `args` assume `exec`.** Isolates load modules, not binaries.
2. **No `compatibility_date`.** On Workers, omitting it is catastrophic (API
   upload defaults to `2021-11-02`). Dynamic Workers require an explicit date on
   every `WorkerCode`. A Bookclerk manifest that cannot name a date cannot be
   loaded safely.
3. **No `compatibility_flags`.** Python guests need `python_workers`; many npm
   packages need `nodejs_compat` / finer `nodejs_*` flags. These are not optional
   polish — they change which language runtimes and APIs exist inside the isolate.
4. **`[sandbox].network` is the wrong abstraction.** workerd’s control plane is
   `globalOutbound`: inherit parent, `null` (deny), or a **service stub** that
   intercepts `fetch`/`connect`. A three-value Landlock enum cannot express
   “all egress through a host proxy with per-plugin props,” which is the
   correct default for untrusted storefronts.
5. **No module graph.** `WorkerCode.modules` needs named modules (`.js` / `.py`
   / `{js|cjs|py|text|data|json}`). The install layout must ship that graph and
   the manifest must point at `main_module`.
6. **No limits surface.** Dynamic Workers support `limits: { cpuMs, subRequests }`
   on the code object and again on `getEntrypoint()`. Manifests should *request*
   ceilings; the host clamps.
7. **`api_version` alone is underspecified.** Wire framing (`jsonrpc-stdio-v1`)
   and isolate ABI (Workers RPC entrypoint methods) are different axes. Collapse
   them into one explicit `abi` string.
8. **Capability widening is invisible.** Anything the guest can put in
   `plugin.toml` it can lie about. Filesystem roots, Cloudflare resource IDs,
   `allowExperimental`, and raw `env` stubs must never be author-controlled.

## Design principles

1. **`plugin.toml` is install-time Worker intent**, not user settings.
   Operator knobs stay in `config.toml` and are injected at handshake / into
   host-built `env` props.
2. **Map 1:1 to `WorkerCode` fields the publisher owns**; host synthesizes the
   rest (`globalOutbound`, `env`, `tails`, `allowExperimental`).
3. **Fail closed on security-relevant typos** (`deny_unknown_fields` everywhere
   capability-related).
4. **Never inherit parent network by omission.** Missing
   `[capabilities.network]` means `deny`, not “whatever the supervisor has.”
5. **Isolate cache identity is content-addressed.** Loader `get(id)` keys must
   change when modules, compat date/flags, or ABI change.
6. **One guest language runtime per plugin.** JS (ESM default) or Python; no
   dual `command` + modules hybrid in v1.
7. **Shared ABI is Workers RPC**, not stdio. Method names can mirror today’s
   JSON-RPC verbs so DTOs stay familiar.

## Proposed install layout

```text
$BOOKCLERK_FILES_DIR/plugins/<id>/
  plugin.toml                 # this schema
  modules/                    # WorkerCode.modules (paths relative to modules/)
    index.js                  # or worker.py
    lib/...
  static/                     # optional text/data/json modules (if declared)
  receipt.json                # install digests / coordinate (host-written)
```

The host reads `plugin.toml`, loads every declared module from disk into a
`modules` map, and calls:

```js
env.LOADER.get(cacheId, () => ({
  compatibilityDate: manifest.runtime.compatibility_date,
  compatibilityFlags: manifest.runtime.compatibility_flags,
  mainModule: manifest.runtime.main_module,
  modules: loadedModules,
  limits: clamped(manifest.runtime.limits),
  globalOutbound: hostEgressFor(manifest),
  env: hostBindingsFor(manifest, config),
  tails: [ctx.exports.PluginTail({ props: { pluginId } })],
}));
```

`cacheId` = `sha256(id + "@" + package_version + "|" + compat + "|" + flags + "|" + moduleDigests + "|" + abi)`.

## Proposed `plugin.toml` schema

### Full example (TypeScript/JS integration)

```toml
schema_version = 1

id = "echo"
name = "Echo Integration"
kind = "integration"          # source | integration | output | database
abi = "bookclerk-rpc/1"       # shared Workers-RPC contract (semver major)

version = "1.2.3"             # plugin package version (also in receipt)

[runtime]
# Required. Same meaning as Wrangler / WorkerCode.compatibilityDate.
compatibility_date = "2026-08-01"

# Optional. Same meaning as WorkerCode.compatibilityFlags.
# Host maintains an allowlist; unknown or experimental flags are rejected
# unless the supervisor itself opts into experimental (dev only).
compatibility_flags = ["nodejs_compat"]

# Required. Must exist in [modules] / modules_dir.
main_module = "index.js"

# Directory of modules relative to plugin.toml (default "modules").
modules_dir = "modules"

# Entrypoint export name for getEntrypoint().
# "default" → default export; otherwise a named WorkerEntrypoint class.
entrypoint = "default"

[runtime.limits]
# Optional requests; host applies min(request, host_cap).
cpu_ms = 30000
subrequests = 100

# Explicit module list (recommended). If omitted, host packs every file under
# modules_dir with a recognized extension. Explicit is preferred for digests
# and for non-extension types (text/data/json).
[[modules]]
name = "index.js"
path = "index.js"
type = "js"                   # js | cjs | py | text | data | json

[[modules]]
name = "lib/util.js"
path = "lib/util.js"
type = "js"

[capabilities.network]
# Required section. Maps to WorkerCode.globalOutbound.
#   deny        → globalOutbound = null
#   host_proxy  → globalOutbound = host EgressProxy stub (filtered fetch/connect)
#   listen is NOT expressible inside an isolate; OAuth callbacks are host-owned
mode = "host_proxy"

[capabilities.bindings]
# Names of Bookclerk host stubs the guest expects on env.*.
# Host wires WorkerEntrypoint loopbacks; publishers never pass resource IDs.
config = true                 # read-only handshake config object
secrets = false               # sealed credential handle API (source login)
plugin_kv = true              # per-plugin durable state (DO facet or disk-backed)
work_fs = false               # ephemeral fetch/upload directory streams
oauth = false                 # host-managed OAuth session API (no guest listen)

[capabilities.methods]
# Declared RPC surface for discovery/help without invoking the guest.
# Authoritative at runtime via handshake / describe.
list = ["handshake", "health", "diagnose", "cli"]

[cli]
[[cli.commands]]
name = "ping"
about = "Probe echo plugin"
[[cli.commands.args]]
name = "message"
long = "message"
kind = "string"
default = "hi"
```

### Python source example (flags matter)

```toml
schema_version = 1
id = "example_py"
kind = "source"
abi = "bookclerk-rpc/1"
version = "0.1.0"

[runtime]
compatibility_date = "2026-08-01"
compatibility_flags = ["python_workers"]
main_module = "worker.py"
modules_dir = "modules"
entrypoint = "Default"

[[modules]]
name = "worker.py"
path = "worker.py"
type = "py"

[capabilities.network]
mode = "host_proxy"

[capabilities.bindings]
config = true
secrets = true
plugin_kv = true
work_fs = true
oauth = true

[capabilities.methods]
list = [
  "handshake",
  "health",
  "login.start",
  "login.complete",
  "scan",
  "fetch_title",
]
```

### Minimal offline transformer

```toml
schema_version = 1
id = "normalize"
kind = "integration"
abi = "bookclerk-rpc/1"
version = "1.0.0"

[runtime]
compatibility_date = "2026-08-01"
main_module = "index.js"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
```

## Field reference

### Top level

| Field | Required | Notes |
| --- | --- | --- |
| `schema_version` | yes | Manifest schema integer; start at `1` |
| `id` | yes | Same rules as today (2–32, `[a-z0-9_]`) |
| `name` | no | Display name |
| `kind` | yes | `source` \| `integration` \| `output` \| `database` |
| `abi` | yes | Must be `bookclerk-rpc/<major>`; host rejects unknown majors |
| `version` | yes | Semver string; feeds loader cache id + receipt |

**Removed vs today:** `command`, `args`, `protocol`, `api_version`, top-level
`[sandbox]`.

### `[runtime]` → `WorkerCode`

| Field | `WorkerCode` | Rules |
| --- | --- | --- |
| `compatibility_date` | `compatibilityDate` | **Required.** `YYYY-MM-DD`. Host may refuse dates newer than the bundled workerd build understands, or older than a configured floor. |
| `compatibility_flags` | `compatibilityFlags` | Optional list. Host **allowlists** known production flags (`nodejs_compat`, `nodejs_als`, `python_workers`, …). Flags that require `allowExperimental` are rejected in production manifests. |
| `main_module` | `mainModule` | Required; must match a module `name`. |
| `modules_dir` | (loader input) | Default `modules`. |
| `entrypoint` | `getEntrypoint(name)` | Default `default`. |
| `limits.cpu_ms` | `limits.cpuMs` | Optional request; host clamps. |
| `limits.subrequests` | `limits.subRequests` | Optional request; host clamps. |

**Never in the manifest:**

| `WorkerCode` field | Why host-only |
| --- | --- |
| `allowExperimental` | Dev supervisor flag; enabling it from a plugin would let guests opt into unstable runtime surface |
| `globalOutbound` | Constructed from `[capabilities.network]` |
| `env` | Constructed from `[capabilities.bindings]` + operator config |
| `tails` | Host observability policy |
| Raw module **contents** | Loaded from disk; digests live in `receipt.json` |

### `[[modules]]`

| Field | Notes |
| --- | --- |
| `name` | Key in `WorkerCode.modules` (import path) |
| `path` | Relative to `modules_dir` |
| `type` | `js` \| `cjs` \| `py` \| `text` \| `data` \| `json` |

Plain-string shorthand is allowed only for `.js` / `.py` names when packing; the
manifest should still prefer explicit `type` so CommonJS and assets are
unambiguous.

### `[capabilities.network]`

| `mode` | Host behavior |
| --- | --- |
| `deny` | `globalOutbound = null` |
| `host_proxy` | `globalOutbound = EgressProxy` stub; host filters URL/scheme/port, injects timeouts, redacts secrets, never exposes raw sockets |

There is **no** `inherit` and **no** `listen`. Loopback OAuth stays a host
binding (`capabilities.bindings.oauth = true`) that returns authorization codes
to the guest after the operator completes the browser flow.

### `[capabilities.bindings]`

Boolean requests for named host stubs. The host either provides the stub or
refuses to load the plugin. Publishers cannot name arbitrary Cloudflare KV/R2/D1
IDs — those are infrastructure behind the host stubs, not guest config.

Suggested initial stub set:

| Binding | Purpose |
| --- | --- |
| `config` | Handshake settings from `config.toml` |
| `secrets` | Put/get sealed store credentials via host DEK |
| `plugin_kv` | Small durable per-plugin state |
| `work_fs` | Stream/FD-like API for acquire/upload work dirs |
| `oauth` | Host-owned OAuth start/complete |

### Shared ABI (`abi = "bookclerk-rpc/1"`)

Guest default (or named) entrypoint exposes Workers RPC methods. Conceptual
parity with today’s JSON-RPC verbs:

- Common: `handshake`, `health`, `diagnose`, `cliDescribe`, `cliInvoke`
- Integration: `start`, `onEvent`, `scanLibrary`, …
- Source: `login`, `loginStart`, `loginComplete`, `scan`, `fetchTitle`
- Output / database: mirror current storage/DB RPCs

Payloads remain JSON-serializable DTOs (structured-clone friendly). Large media
moves through `work_fs` streams, not RPC line payloads.

Versioning:

- `abi` major bump = breaking RPC/DTO change
- `compatibility_date` / flags = V8/workerd behavior, orthogonal to Bookclerk ABI
- `schema_version` = `plugin.toml` shape only

## Host validation rules (normative)

1. Reject manifests missing `compatibility_date`, `main_module`, `abi`, `kind`,
   `id`, `version`, or `[capabilities.network]`.
2. Reject unknown keys under `[runtime]`, `[capabilities.*]`, and `[[modules]]`.
3. Reject `compatibility_flags` not on the host allowlist.
4. Reject any flag that requires `allowExperimental` unless
   `BOOKCLERK_PLUGIN_ALLOW_EXPERIMENTAL=1` **and** the supervisor Worker itself
   has `"experimental"` (local dev only).
5. Refuse to load if `compatibility_date` is newer than the embedded workerd’s
   known latest date.
6. Default network to **deny** is not reachable: the section is required, so
   omission is a parse error (same spirit as today’s typo-hostile sandbox parse).
7. Widening capabilities on upgrade (`deny` → `host_proxy`, new bindings) requires
   an explicit operator approval step (same idea as `--approve-capabilities`).
8. Module digests in `receipt.json` must match bytes on disk before `LOADER.get`.

## Mapping summary

```text
plugin.toml                         WorkerCode / loader
─────────────────────────────       ────────────────────────────────
runtime.compatibility_date     →    compatibilityDate
runtime.compatibility_flags    →    compatibilityFlags
runtime.main_module            →    mainModule
[[modules]] + modules_dir      →    modules
runtime.limits.*               →    limits (clamped)
capabilities.network           →    globalOutbound (host-built)
capabilities.bindings          →    env (host-built stubs)
(host policy)                  →    tails, allowExperimental
abi + RPC methods              →    entrypoint Workers RPC
id@version+digests+compat      →    LOADER.get(cacheId)
```

## What stays out of `plugin.toml`

- Operator enablement and opaque knobs (`config.toml`)
- Cloudflare account bindings / resource ids
- Filesystem allowlists (isolates do not see the host FS; `work_fs` is mediated)
- Native jail knobs (`bookclerk-jail`, Landlock, AppContainer)
- Publisher signing material (receipt / catalog)
- Build tooling (`wrangler.toml` may exist **in the plugin source repo** for
  authors; the **installed** artifact is `plugin.toml` + `modules/`, not a
  Wrangler project)

Author workflow tip: keep a `wrangler.toml` for typegen (`wrangler types`) and
local playgrounds, then emit `plugin.toml` + bundled `modules/` as the install
artifact. Do not ask operators to run Wrangler.

## Implications for native-heavy guests

Audible DRM, SQLite engines, and codec work do not fit comfortably inside a
JS/Python isolate. Greenfield options (product decision, not this schema):

1. **Keep a separate native helper** invoked only through host bindings (media
   worker / sealed decrypt service), while the *plugin* itself is still an
   isolate that orchestrates API calls; or
2. **Carve a second runtime** later — but do **not** reintroduce `command` into
   this manifest. If a native runtime returns, give it its own manifest kind
   (`runtime.engine = "native"`) rather than overloading the workerd schema.

This proposal assumes **workerd is the guest runtime** for third-party plugins.

## Open product decisions

1. Supervisor topology: embed workerd in `bookclerkd`, spawn a companion
   workerd process, or call out to Cloudflare-hosted Dynamic Workers for
   cloud-synced installs?
2. Floor/ceiling policy for `compatibility_date` relative to the shipped workerd
   version.
3. Whether `plugin_kv` is a Durable Object facet (cloud) or a host-disk stub
   (local-first daemon).
4. Exact Workers RPC method naming (`fetch_title` vs `fetchTitle`) and DTO
   package location.
5. Allowlist of compatibility flags for the first release.

## Implementation sketch (later)

1. Replace `PluginManifest` in `bookclerk-plugin-host` with the schema above.
2. Add a workerd supervisor crate/binary that owns `WorkerLoader` and host stubs.
3. Port `examples/plugins-echo-ts` to emit `modules/index.js` + this manifest.
4. Rewrite `docs/plugins.md` / `docs/plugin-registry.md` against the new layout.
5. Defer native storefronts until host bindings for secrets + `work_fs` exist.

## References

- [Dynamic Workers](https://developers.cloudflare.com/dynamic-workers/)
- [Worker Loader / `WorkerCode` API](https://developers.cloudflare.com/dynamic-workers/api-reference/)
- [Custom limits](https://developers.cloudflare.com/dynamic-workers/usage/limits/)
- [Compatibility dates](https://developers.cloudflare.com/workers/configuration/compatibility-dates/)
- [Compatibility flags](https://developers.cloudflare.com/workers/configuration/compatibility-flags/)
- [Wrangler configuration](https://developers.cloudflare.com/workers/wrangler/configuration/)
