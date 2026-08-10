# Dynamic plugins

Bookclerk is built around pluggable **sources**, **destinations**, and
**integrations**. First-party **guest** implementations live under
`crates/bookclerk-plugins/` (one package per plugin). The **host** runtime is
[`bookclerk-plugin-host`](../crates/bookclerk-plugin-host) — discovery, spawn,
jail, consent, and Workers RPC — distinct from the runtime install tree
`$BOOKCLERK_FILES_DIR/plugins/`.

The product ABI is **Workers RPC** at `api_version = 1` (no `protocol` key; no
JSON-RPC 2.0 as the product ABI). Decision record:
[`docs/adr/plugin-workers-rpc-workerd.md`](adr/plugin-workers-rpc-workerd.md).
Authoritative schema:
[`crates/bookclerk-plugin-abi/schema/abi.json`](../crates/bookclerk-plugin-abi/schema/abi.json).

Authors implement the branded guest base **`BookclerkPlugin`**:

| Language | Package |
| --- | --- |
| TypeScript (workerd) | [`@bookclerk/plugin-sdk`](../packages/plugin-sdk/) |
| Rust (native) | [`bookclerk-plugin-sdk`](../crates/bookclerk-plugin-sdk/) |

Runtimes in `plugin.toml`:

| `runtime` | How it runs |
| --- | --- |
| `native` | OS binary speaking the same Workers RPC ABI (spawned through `bookclerk-jail`) |
| `workerd` | Author `modules/` loaded by first-party [`bookclerk-workerd`](../crates/bookclerk-workerd/) — **one jail + one isolate per plugin** (local embed, not Cloudflare cloud execution) |

For the product overview see the [documentation index](README.md). Built-in
storefronts: [sources.md](sources.md). Audiobookshelf / Connect:
[integrations.md](integrations.md). Publishing / crates.io taxonomy and
standalone author repos: [plugin-registry.md](plugin-registry.md).

## Terminology

| Term | Meaning |
| --- | --- |
| **Plugin host** | Crate `bookclerk-plugin-host`; loads guests, never ships storefront logic by default |
| **Guest / external plugin** | Separate process (native binary or `bookclerk-workerd`) + `plugin.toml` under `plugins/<id>/` |
| **Plugin package** | Rust crate under `crates/bookclerk-plugins/` (or third-party repo), or a workerd archive (`plugin.toml` + `modules/`) |
| **Platform-shipped guest** | First-party external plugin bundled in the install package (`plugins/sqlite/`, `plugins/local/`, storefronts, …) — sandboxed subprocess, not linked in-process |
| **In-process fallback** | When a platform guest is missing or fails to start, hosts fall back to the same logic in `bookclerk-library` (SQLite) or `bookclerk-storage` (local output) |
| **`bundled-plugins`** | Optional host feature linking storefronts in-process (dev only; omit for release packaging) |
| **`BookclerkPlugin`** | Branded guest base (TS class / Rust trait); app code never depends on bare platform entrypoints |

## Local development (external guests)

Hosts default to **external guests only** (no storefronts linked in-process).
One command builds, stages, and runs with sandboxed platform guests (sqlite, local,
storefronts):

```bash
export BOOKCLERK_FILES_DIR=/tmp/BookclerkFiles   # optional; this is the default
cargo dev-daemon                                 # daemon with staged guests
cargo dev-cli -- version                         # CLI with staged guests
```

Granular aliases (same dispatcher — see `crates/bookclerk-dev/README.md`):

```bash
cargo build-plugins          # guest binaries only
cargo stage-plugins          # build + copy to target/plugin-artifacts
cargo test-staged            # build + stage + handshake integration test
```

Add `--release` to any alias for release builds. Override staging dir with
`BOOKCLERK_PLUGIN_ARTIFACTS`. Forward host args after `--` (e.g. `cargo dev-daemon -- --help`).

Optional in-process iteration (no staging): build hosts with
`--features bundled-plugins` on `bookclerk-cli` / `bookclerkd`.

Workerd Echo (TypeScript):

```bash
cd packages/plugin-sdk && npm ci && npm run build
cd ../../examples/plugins-echo-workerd && npm ci && npm run typecheck
```

Native Echo: `crates/bookclerk-plugin-examples/echo-integration`.

## Why subprocesses?

Content sources and integrations need their own async runtimes, HTTPS clients,
and OAuth flows. Loading foreign `cdylib`s into the host process is fragile
across Rust/Tokio versions. A child process (native binary or
`bookclerk-workerd` under jail) gives crash isolation, independent releases, and
a stable ABI. Workerd is an **authoring/runtime** choice for portable JS/TS
modules — the jail remains the security boundary (see the ADR).

## Trust model (external plugins are untrusted)

External plugins run as a **separate OS process**, confined by the host to the
directories it hands them ([the guest jail](#the-guest-jail)). The ABI boundary
is narrow on top of that:

| Host guarantees | Detail |
| --- | --- |
| No library DB path (sources / integrations / outputs) | `library.db` is never passed on the wire to those kinds — and not reachable if it were. **Database** guests receive the SQLite file only via the fd-3 side channel (or `sqlite_path` when unconfined) at `dbConnect` |
| No files-dir root | Plugins get `plugin_data_dir` (`…/plugins/<id>/data`) and a per-fetch work directory (descriptor) — not `master.key` or the download cache root |
| Env scrub | Child spawn uses `env_clear` + a small allowlist (`PATH`, locale, …). `BOOKCLERK_*`, `AWS_*`, tokens, and DB URLs are not inherited; `HOME` and `TMPDIR` are replaced with the guest's own directories |
| Host-mediated secrets | `login` returns `{ account, credentials }`; host seals into `encrypted_secrets` with `provider = plugin id`. `scan` and `fetchTitle` receive those blobs from the host |
| Host-mediated library writes | `scan` returns book DTOs; host upserts with `source` forced to the plugin id. Account listing for a plugin id is answered from the host accounts table |
| Scoped identity | Plugin cannot claim another storefront’s `source` / `provider` |
| Network consent | Operator runs `bookclerk plugins approve` before enable; only declared `capabilities.network.domains` may be contacted as **initial** request hosts. Redirect hops do **not** need re-allowlist membership |

First-party guests ship under `crates/bookclerk-plugins/` with the guest SDK
contract. Host binaries (`bookclerk`, `bookclerkd`) depend on
**`bookclerk-plugin-host`** only — not on individual store crates. Optional
`bundled-plugins` features on the hosts call `register_builtin_*` to link
first-party libraries in-process for faster Rust iteration; release builds omit
that feature and load staged guests from `plugins/` instead. Discovered
external copies of the same id are skipped when an in-process adapter is already
registered. After registration, hosts talk **only**
through `ContentSource` /
`Integration` (login, scan, fetch, import, revoke, inspect, plus catalog
`searchCatalog` / `catalogDetail` / `expandCandidates` / `purchaseHint` /
`listDeals` for Discover). Sources always return `PlainFetch` (`SourceFetch` is
an alias) —
DRM (Adrm/CENC) is decrypted inside the Audible plugin before the host sees
media. Guest `fetchTitle` carries optional `pdfUrl`; catalog methods are on the
Workers RPC wire for external guests.

Enabling a third-party plugin still means running that guest as the Bookclerk
user, inside the jail below — review and **approve** capabilities before
enabling.

## Shipping without a store

Both hosts carry one optional feature per in-process plugin (`bundled-plugins`
enables the full set). **Default builds link no storefront** (external guests
only):

```bash
# Default: external guests only (release packaging).
cargo build --release -p bookclerk-cli -p bookclerkd

# Everything except Audible (still in-process, opt-in).
cargo build -p bookclerk-cli -p bookclerkd --features bundled-plugins \
  --no-default-features \
  --features bookclerk-plugin-source-libro,bookclerk-plugin-source-chirp,bookclerk-plugin-source-graphicaudio,bookclerk-plugin-integration-audiobookshelf
```

This exists for Audible specifically. Adrm and Widevine CENC decrypt live in that
plugin, and some regions restrict distributing a binary that can circumvent
DRM — so whoever packages Bookclerk needs the option to ship hosts that contain
no such code at all, rather than a build flag that merely disables it at runtime.
Omitting the feature omits the crate, and with it the ciphers, the content-key
handling, and the CDM.

Nothing else has to move for that to hold. The shared MP4 plumbing
(`bookclerk-mp4`) parses and rewrites containers and takes a `SampleTransform`
from its caller; the Audible plugin's transform is the only one that decrypts.
`scripts/check-store-free-hosts.sh` asserts it in CI: default hosts must link
no plugin package and reach no cipher crate. Opt-in `--features bundled-plugins`
must still link Audible for in-process dev. A shared crate that grew an `aes`
dependency fails the default-host check.

Users of a store-free build can still add any storefront back as an external
guest, since discovery is independent of these features. That is a deployment
choice made by whoever installs the plugin, not by whoever shipped the binary.

## The guest jail

Every external guest is started by **`bookclerk-jail`**, a small launcher that
applies a confinement policy to itself and then `exec`s the plugin process
(native binary **or** `bookclerk-workerd`). What it grants is decided entirely
by the host. Workerd is not a substitute for the jail — one jail + one isolate
per plugin.

A guest gets four paths and nothing else:

| Path | Access | Also known to the guest as |
| --- | --- | --- |
| its install directory | read-only | `cwd` |
| `…/plugins/<id>/data` | read/write | `HOME`, and `plugin_data_dir` on the wire |
| `…/plugins/<id>/tmp` | read/write | `TMPDIR` / `TEMP` / `TMP` |
| one fetch work directory at a time | write (via descriptor) | passed on fd 3 immediately before each `fetchTitle` |

Plus the system read paths every process needs to start (the loader, shared
libraries, the CA bundle — including `/var/lib/ca-certificates` on
openSUSE/SLE where `/etc/ssl` only symlinks there — resolver config) and a
writable `/dev/null`.

That leaves out `master.key`, `library.db`, `config.toml`, the operator token,
the finished library, the download cache root, and every *other* plugin's data
directory. None of it is a loss: credentials arrive as RPC parameters and scan
results go back the same way, so a guest has never had a reason to open the
database.

`TMPDIR` and `HOME` are **replaced**, not inherited. The values a host process
carries name directories outside every jail, so a guest reaching for a temp file
the ordinary way would fail on a permission error unrelated to anything it was
denied. `XDG_RUNTIME_DIR` is dropped for the same reason and has no per-guest
equivalent to point at.

### Why a descriptor per fetch rather than the cache root

A guest is long-lived — one process per plugin, serving every call for the life
of the daemon — and filesystem confinement is fixed at spawn. Granting the whole
cache would let one fetch read or overwrite every other fetch's scratch.

The host therefore opens exactly one work directory per `fetchTitle`, sends it
over a Unix socket with `SCM_RIGHTS` on fd 3 (preserved through
`bookclerk-jail`), and the guest resolves `/proc/self/fd/N` (or `/dev/fd/N` on
macOS), where `N` is the received directory descriptor. The `cache_dir` string
on the wire remains for logging and for unconfined development; jailed guests
must use the descriptor.

A media job is confined far more tightly: one input file, one output directory,
per job, in a process that exits when the job does. See [media.md](media.md).

### Why a launcher and not self-confinement

`bookclerk-media-worker` confines itself, which is fine because that binary is
ours: only our own code could skip the call. A plugin guest inverts the
assumption — the guest binary *is* the untrusted part, so asking it to confine
itself asks the attacker to cooperate.

The host cannot apply the jail directly either. Both backends allocate, which is
unsafe after `fork` in a threaded process, and Landlock's `restrict_self` binds
the calling thread rather than the process, so a runtime's worker threads would
stay unconfined. `bookclerk-jail` is single-threaded and does nothing else, so it
has neither problem: it applies the policy, `exec`s the guest, and the guest
inherits restrictions it cannot refuse or inspect.

The policy crosses that boundary as JSON in `BOOKCLERK_JAIL_SPEC`, which the
launcher removes before the hand-off. A missing, malformed, or unsatisfiable spec
means **nothing runs** — the guest binary is never reached.

One consequence is worth stating plainly: the policy has to permit `execve`,
because that is how control is handed over. A guest can therefore run other
binaries inside its read allowlist, but it gains nothing by doing so. The
restrictions are inherited, irreversible, and `no_new_privs` is set, so setuid is
already neutral.

### Open descriptors, and why the launcher sweeps them

The allowlist is about paths, and an open descriptor is past the path check for
good. A guest reads one with no lookup at all, so a file the host still had open
across the spawn would be readable inside the jail whatever the policy said —
`master.key` included — and no grant could take it back.

So the launcher closes every descriptor above stdin, stdout and stderr before it
applies the policy, and refuses to hand over at all if it cannot enumerate them.
Nothing leaks today, because Rust opens files `O_CLOEXEC`; the point is that this
stops being a property of every library the host links, re-checked on every
dependency bump, and becomes a property of the jail.

### Declaring what a plugin needs

A manifest declares network and host bindings under **`[capabilities]`** — not
a free-form filesystem widen:

```toml
[capabilities.network]
mode = "outbound"   # deny | outbound
domains = ["api.example.com", "www.example.com"]

[capabilities.bindings]
config = true
secrets = true
work_fs = true
oauth = true          # interactive OAuth; jail maps to loopback listen
# plugin_kv = true    # workerd guests often want durable per-plugin KV
```

| Network `mode` | Grants |
| --- | --- |
| `deny` (default when omitted from older drafts — prefer explicit) | no IP sockets at all |
| `outbound` | outbound via the host egress path; **initial** hosts must be listed in `domains` |

`domains` is required when `mode = "outbound"`. The operator approves that list
before enable. The host follows HTTP redirects by default; **redirect hops do
not need to be on the allowlist**. A direct request to a non-listed host is
denied.

`bindings.oauth = true` (with outbound network) is how storefronts declare an
OAuth-style callback need. The jail maps that to outbound-plus-loopback listen;
the **host** still owns the browser-facing TCP listener and forwards connections
over IPC (see [Interactive listeners](#interactive-listeners-oauth-and-similar)).

Unrestricted network access is deliberately not expressible, and **the filesystem
allowlist cannot be widened from a manifest**. A manifest ships with the plugin
it describes, so anything it can ask for is something a hostile plugin can ask
for too — consent and review are the operator controls.

Optional `[capabilities.methods]` lists RPC method names for discovery/consent
UI (e.g. `handshake`, `health`, `onEvent`, `cli`).

### Isolation modes

```toml
[plugins]
isolation = "required"  # required | best-effort | off
# jail_bin = "/usr/local/bin/bookclerk-jail"
```

Environment overrides: `BOOKCLERK_PLUGIN_ISOLATION`, `BOOKCLERK_PLUGIN_JAIL`.

- **`required`** (default) — a plugin that cannot be jailed is not loaded. The
  error names the reason and the plugin is skipped; the rest of the host runs.
- **`best-effort`** — start the guest unconfined when the platform or the
  installation cannot support a jail, with a warning that says so per plugin.
- **`off`** — no jail. Development only.

The same three modes as `[media].isolation`, and the same reasoning: the tiers
differ in what they reach for, not in how a missing jail is handled. Confirm
what is in effect with `bookclerk config get plugins.isolation`.

| Platform | Backend | Filesystem | Syscalls | Network |
| --- | --- | --- | --- | --- |
| Linux | Landlock + seccomp-bpf | allowlist, ABI-probed | deny list | per-policy |
| macOS | Seatbelt (`sandbox_init`) | deny-default SBPL profile | — | per-policy |
| Windows | AppContainer | spawn-time ACL allowlist | — | capability SIDs |

### Windows confinement

Windows cannot confine a process after it has started. `bookclerk-jail` therefore
`CreateProcess`es the guest into an AppContainer (via
`bookclerk_sandbox::spawn::run_appcontainer`), ACLs the policy's **explicit**
read/write paths for a **per-launch** Package SID, maps `NetPolicy` to
capability names (`internetClient`, `privateNetworkClientServer`, …), places the
guest in a kill-on-close Job Object, and proxies stdio until the guest exits.

#### Job Object launch ordering

Bookclerk owns CreateProcess (not rappct’s assign-after-run path). When Job
membership is required, the launcher prefers `PROC_THREAD_ATTRIBUTE_JOB_LIST`
so assignment happens before any guest instruction runs; otherwise it uses
`CREATE_SUSPENDED` → configure Job (`KILL_ON_JOB_CLOSE` + resource limits) →
`AssignProcessToJobObject` → `ResumeThread`. Any failure after CreateProcess
terminates the child and closes process, thread, pipe, and Job handles.
Descendants cannot outlive jail/profile/ACL cleanup.

#### Profile paths

`GetAppContainerFolderPath` is authoritative when under Known Folder
LocalAppData `\Packages\`. Docs describe `Packages\<moniker>\AC`; Windows CI
measures `Packages\<package-SID>` (with `\AC` used when that child exists).
Bookclerk fails closed if the API fails or returns a path outside Packages — it
does **not** synthesize a Packages path. Child cwd / `LOCALAPPDATA` use that
folder; `TEMP`/`TMP` use `<folder>\Temp`.

#### ACL mutations

Bookclerk **never mutates DACLs under protected OS-managed roots** (Windows /
System32 / WinSxS / Program Files / ProgramData\Microsoft, resolved via
`GetWindowsDirectoryW` / `GetSystemDirectoryW` / Known Folders). Ambient OS
runtime access (loading system DLLs / `cmd.exe`) comes from existing OS ACLs
such as ALL APPLICATION PACKAGES. Explicit policy paths are temporarily ACLed
for the Package SID. Cross-process DACL read/modify/write is serialized with the
named mutex `Local\bookclerk-dacl-tx` (plus an in-process lock). Revoking an ACE
does **not** invalidate handles the guest already opened.

Plugin hosts create the AppContainer profile up front, put
`windows_profile_name` on the jail `Spec`, and delete the profile when the
plugin client drops. Media jobs leave that field unset so the jail creates a
unique profile per job.

Per-fetch / upload / sqlite paths are not in the spawn allowlist. On Unix the
host passes an open descriptor over `SCM_RIGHTS`; on Windows it temporarily ACLs
the path for the Package SID, puts the path on the RPC wire, and revokes
the ACE when the RPC returns.

#### Interactive listeners (OAuth and similar)

`capabilities.bindings.oauth = true` declares that the storefront needs an
OAuth-style callback. The **host** owns the browser-facing TCP listener and
forwards each connection over a duplex IPC tunnel (Unix socket under the plugin
scratch dir, or a Windows named pipe) into the guest, which still runs its HTTP
stack (Audible LoginServer). `loginStart` carries `callback_ipc` +
`callback_public_base`; the guest must not bind TCP when those are set.

On Windows the named pipe is created with a Package-SID DACL (plus SYSTEM /
Administrators / Creator Owner) and a Low mandatory integrity label so the
AppContainer guest can open it; remote clients are rejected. Unix sockets stay
mode `0600` under the plugin scratch dir.

This is required on Windows AppContainer (host↔guest loopback is blocked even
with Full caps / CheckNetIsolation) and is used on all OSes for a uniform
contract. Do not enable CheckNetIsolation. `--external` paste flows remain
available as a fallback.

#### Availability

Plugin Jobs get conservative memory / active-process (and optional CPU) limits;
media workers get higher defaults. RPC timeouts and framing violations kill the
guest and quarantine the client until restart. Stdin proxying does not block
jail exit after the guest terminates.

#### Trust vs sandbox

Plugins are untrusted relative to Bookclerk’s master key, database, unrelated
files, and other plugins. A source plugin is necessarily trusted with credentials
and content deliberately sent to it; a network-capable plugin can exfiltrate
anything it was given. The sandbox reduces blast radius; it does not authenticate
publisher intent. SHA-256 digests verify artifact integrity, not publisher
identity — unsigned/unverified installs require explicit approval.

`allow_exec` is not separately enforceable at CreateProcess on Windows; path
ACLs and low integrity remain the boundary. Descendants inherit the AppContainer
token and Job Object membership. Regular AppContainer (not LPAC) is the default
spawn path; LPAC ambient restrictions are not currently applied.

Self-confinement (`Policy::confine_current_process`) remains unsupported on
Windows — media workers use the same spawn-side AppContainer path through
`bookclerk-jail`.

In containers, Landlock needs a runtime that permits the `landlock_*` syscalls.
Docker's default seccomp profile has allowed them since 20.10.14; on an older
engine the syscalls are refused, `required` refuses to load plugins, and the
startup log names Landlock as the missing backend.

### Installing the launcher

The host looks for `bookclerk-jail` in this order:

1. `plugins.jail_bin` in `config.toml`
2. `BOOKCLERK_PLUGIN_JAIL`
3. `bookclerk-jail` beside the running executable

Each candidate is checked for existence, so a stale configured path fails loudly
instead of degrading to an unconfined guest. Build and ship it with the hosts
(and `bookclerk-workerd` when shipping workerd guests):

```bash
cargo build --release -p bookclerk-cli -p bookclerkd \
  -p bookclerk-media-worker -p bookclerk-jail -p bookclerk-workerd
```

The Docker images copy helpers into `/usr/local/bin` alongside `bookclerk` and
`bookclerkd`.

### Troubleshooting

**`refusing to run plugin <id> unconfined: bookclerk-jail not found in <dir>`** —
the launcher is not installed beside the host binary. Install it, point
`plugins.jail_bin` at it, or accept unconfined guests with
`isolation = "best-effort"`.

**`refusing to run plugin <id> unconfined: this host cannot confine a process`** —
no backend: Windows, a kernel without Landlock, or a container runtime that
blocks the Landlock syscalls. The message names the backend that came up short.

**`bookclerk-jail: these allowlist paths do not exist: …`** — a granted directory
vanished between planning and launch. Both backends reject a rule naming a
missing path, and the launcher refuses rather than silently narrowing the jail.

**A plugin fails on a path it used to read** — it is outside the four grants. A
guest that needs host data should be asking for it over RPC; the filesystem
allowlist is not negotiable from a manifest.

**Everything fails right after `handshake`** — check the guest's stderr, which is
inherited by the host and lands in the daemon log directly under the
`bookclerk-jail:` line reporting what was applied.

**Enable refused: run `bookclerk plugins approve` first** — consent grant is
missing or the plugin widened `capabilities` since the last grant. Re-approve,
then `bookclerk plugins enable <id>`.

## Two files, two jobs

| File | Role |
| --- | --- |
| `plugin.toml` (next to the binary or `modules/`) | **Install / discovery** — id, kind, runtime, command or `[workerd]`, capabilities |
| `config.toml` (`[sources.<id>]` / `[integrations.<id>]`) | **User settings** — `enabled`, opaque knobs |

The plugin (or its installer) drops a directory under a search root. Bookclerk
scans for `plugin.toml`, spawns the native `command` or `bookclerk-workerd`, and
passes the matching main-config table on `handshake`. Users never put `command`
in `config.toml`.

## Layout

Native:

```text
$BOOKCLERK_FILES_DIR/plugins/
  echo/
    plugin.toml
    bookclerk-plugin-echo-integration   # executable
    data/                               # host-created: guest state, its HOME
    tmp/                                # host-created: guest scratch, its TMPDIR
```

Workerd (script archive — no per-OS binary required):

```text
$BOOKCLERK_FILES_DIR/plugins/
  echo/
    plugin.toml
    modules/
      index.js
    data/
    tmp/
```

`data/` and `tmp/` are created by the host at spawn, so a plugin archive should
not ship them; deleting one plugin's state means deleting those two directories.
They are keyed by plugin id under `$BOOKCLERK_FILES_DIR/plugins/<id>/` wherever
the guest itself was installed — read-only to the guest apart from that writable
pair.

Additional roots: `BOOKCLERK_PLUGIN_DIRS` (OS path list). A guest staged under one
of those still keeps its state under the files dir, so an upgrade that replaces
the staging tree does not take the plugin's state with it.

### `plugin.toml`

Native Echo:

```toml
api_version = 1
id = "echo"
name = "Echo Integration"
kind = "integration"          # source | integration | output | database
version = "0.1.0"
runtime = "native"
command = "./bookclerk-plugin-echo-integration"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true

[capabilities.methods]
list = ["handshake", "health", "diagnose", "onEvent", "cli"]

# Optional: CLI help without spawning (handshake / cliDescribe win at invoke)
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

Workerd Echo (see [`examples/plugins-echo-workerd/`](../examples/plugins-echo-workerd/)):

```toml
api_version = 1
id = "echo"
name = "Echo Integration"
kind = "integration"
version = "1.0.0"
runtime = "workerd"

[workerd]
compatibility_date = "2026-08-01"
main_module = "index.js"
modules_dir = "modules"
entrypoint = "default"

[capabilities.network]
mode = "deny"

[capabilities.bindings]
config = true
plugin_kv = true
```

There is **no** `protocol` key. `api_version` must be `1`. Settings derives a
Google favicon from the first entry in `capabilities.network.domains` when
present.

`command` (native) may be absolute or relative to the manifest directory. An
absolute `command` is granted read access on its own, so a manifest may point at
a binary installed elsewhere. For `runtime = "workerd"`, the host resolves
`bookclerk-workerd` beside itself (or via config) and loads `[workerd]` +
`modules/`. If `compatibility_date` is newer than the bundled workerd, the host
**warns** and still loads.

Unknown capability keys are parse errors rather than silent defaults — a typo in
a security-relevant field must not read as "whatever we would have picked".

Two plugins that claim the same `id` for the same `kind` are a **hard startup
error** (CLI/daemon exit). The same id across different kinds (e.g. a source and
an integration both named `echo`) is allowed. An external id that collides with
a first-party plugin of the same kind is also rejected.

## Consent before enable

Third-party (and newly installed) plugins require an explicit permission grant:

```bash
bookclerk plugins approve echo          # interactive summary of network/bindings
bookclerk plugins approve echo --yes    # non-interactive
bookclerk plugins enable echo
```

Grants are persisted under the files dir. Widening `capabilities.network` or
bindings after a prior grant requires re-approval. Redirect following does not
expand the consented domain list.

## Enabling and settings in `config.toml`

Plugin `id` must match a config table. **External integrations default to
disabled**; sources follow the usual `[sources.<id>]` rules (missing → enabled).
`plugins enable` still refuses until a grant exists.

```toml
[integrations.echo]
enabled = true
# greeting = "hi"   # opaque knobs → handshake config

[sources.my_store]
enabled = true
# … opaque knobs …
```

## ABI (api_version = 1, Workers RPC)

Host ↔ plugin: Workers RPC method calls with structured (camelCase) payloads.
The schema in
[`crates/bookclerk-plugin-abi/schema/abi.json`](../crates/bookclerk-plugin-abi/schema/abi.json)
is authoritative; Rust (`bookclerk-plugin-abi` / `bookclerk-plugin-sdk`) and
TypeScript (`@bookclerk/plugin-sdk`) are generated projections of that contract.
Method names on the wire are **camelCase** (`onEvent`, `cliInvoke`, `fetchTitle`,
…). Guests outside the host’s supported API version fail handshake cleanly.

Native guests typically speak the ABI over stdio framing provided by
`bookclerk-plugin-sdk::serve_native`. Workerd guests expose the same methods on
a `BookclerkPlugin` entrypoint loaded by `bookclerk-workerd`.

### Common

| Method | Purpose |
| --- | --- |
| `handshake` | Negotiate version, id, kind, capabilities, brand |
| `shutdown` | Graceful teardown |
| `health` | Connectivity / config check |
| `diagnose` | Human-readable CLI probe lines |
| `cliDescribe` | Declared CLI command schema (`CliSchema`) |
| `cliInvoke` | Run a declared command (`CliInvokeParams` → `CliInvokeResult`) |

Handshake params include `{ "apiVersion": 1, "config": {…} }` — the plugin’s
`[sources.<id>]` / `[integrations.<id>]` table from **main** `config.toml` as JSON
(empty object if the table is missing).

Optional handshake field `cli` may embed the same schema as `cliDescribe`. Prefer
advertising capability `cli` and implementing both methods. You may also mirror
the schema in `plugin.toml` under `[cli]` so `bookclerk plugins <id> --help`
works without spawning the plugin; at invoke time handshake / `cliDescribe`
remain authoritative.

### Plugin CLI

Host commands that apply to every plugin stay on Bookclerk verbs
(`plugins list|info|diagnose|approve|enable|disable`, `integrations …`,
`auth …`). Plugin-specific commands are declared and invoked as:

```bash
bookclerk plugins <plugin-id> <command> [args…]
```

Example schema (JSON / handshake `cli` / `cliDescribe`):

```json
{
  "commands": [
    {
      "name": "ping",
      "about": "Probe echo plugin",
      "args": [
        {
          "name": "message",
          "long": "message",
          "kind": "string",
          "required": false,
          "default": "hi"
        }
      ]
    }
  ]
}
```

`cliInvoke` params: `{ "command": "ping", "args": { "message": "hi" } }`.
Result: `{ "exitCode": 0, "stdout": "…", "stderr": "…", "json": … }`.

### Integration capabilities

Advertise in `handshake.capabilities`: `start`, `onEvent`, `health`,
`diagnose`, `scanLibrary`, `syncListening`, `authenticateUser`, `cli`.

| Method | Notes |
| --- | --- |
| `start` | Background watchers |
| `onEvent` | `{ "type": "book_acquired"\|"external_user_observed", "payload": … }` |
| `scanLibrary` | `{ "force": bool }` |
| `syncListening` | Return listening progress snapshots; host upserts tagged with plugin id |
| `authenticateUser` | `{ "username", "password" }` → external user |
| `pollEvents` | Return observed external users — host polls after `start` and kicks off **core** workflows (e.g. claim tickets). The plugin stays oblivious to portal/tickets |

### Source capabilities

| Method | Notes |
| --- | --- |
| `login` | Password sources. Params include `pluginDataDir`, marketplace/label/email/password. Result: `{ account, credentials? }` — host seals credentials (`provider = plugin id`) and upserts the account row |
| `loginStart` / `loginComplete` | OAuth sources (Audible). Start returns `{ sessionId, url }`; complete returns login result |
| `scan` | Params include `pluginDataDir`, filters, and host-injected `credentials` map (`accountId` → opaque JSON; **no** library DB path). Result includes `books[]` DTOs; host upserts with `source` forced to plugin id |
| `fetchTitle` | Host injects `credentials` from `encrypted_secrets`; plugin writes media under the work directory and returns **plain** paths (DRM guests decrypt before return) |

Plugins must not open `library.db` or read `master.key`. Do not put Encrypted
content keys on the wire — decrypt in the guest when needed.

### Output plugins

`kind = "output"` guests implement storage over Workers RPC. The host
never grants them the acquire cache or output library: `putFile` delivers the
local media file over the same side channel as source `fetchTitle` directories
(fd 3 is the preserved `SCM_RIGHTS` socket; the open file descriptor arrives
over that socket immediately before the RPC). When no side channel is wired
(unconfined / best-effort), the host sends an absolute local path in the RPC
params instead. S3 credentials and bucket config are injected on each RPC —
guests do not inherit `BOOKCLERK_AWS_*` or read `encrypted_secrets`.

| Method | Notes |
| --- | --- |
| `put` | Small objects (covers, sidecars): key + data + meta |
| `putFile` | Large audiobooks: host passes file fd, then RPC key + meta |
| `get` / `exists` / `list` / `probe` / `copy` / `delete` / `touchFile` | Mirror in-process storage |

First-party S3 ships as `bookclerk-plugin-destination-s3`. When the guest is
discovered under `plugins/s3/` and `[output.s3].enabled = true`, the host
loads it at startup via external destination loading instead of the in-process
S3 backend.

### Database plugins

`kind = "database"` guests implement the SeaORM proxy boundary over Workers RPC.
Engine connect/migrate/proxy code lives in the guest
(`bookclerk-plugin-database` modules); the host does not link SQL engines.
The host opens the library through the external database loader (guest required —
no in-process fallback). SQLite receives `library.db` on fd 3 at `dbConnect`.

| Method | Notes |
| --- | --- |
| `dbConnect` | Open backend via tagged connect params (`backend`: `sqlite` / `d1` / `postgres`); returns dialect (SQLite: fd 3; D1/Postgres: host-injected credentials) |
| `dbPing` | Verify connectivity |
| `dbQuery` / `dbExecute` | Forward SeaORM statement payloads |

Built-in ids: `sqlite`, `d1`, `postgres` (match `[database].plugin`).

## Examples

Native Echo:

```bash
cargo build -p bookclerk-plugin-echo-integration
mkdir -p "$BOOKCLERK_FILES_DIR/plugins/echo"
cp target/debug/bookclerk-plugin-echo-integration \
  "$BOOKCLERK_FILES_DIR/plugins/echo/"
cp crates/bookclerk-plugin-examples/echo-integration/plugin.toml \
  "$BOOKCLERK_FILES_DIR/plugins/echo/"
```

Workerd Echo — install `plugin.toml` + `modules/` from
[`examples/plugins-echo-workerd/`](../examples/plugins-echo-workerd/) (host
spawns `bookclerk-workerd`, not a SEA binary).

```toml
# config.toml
[integrations.echo]
enabled = true
```

```bash
bookclerk plugins approve echo --yes
bookclerk plugins list
bookclerk plugins enable echo
bookclerk integrations status
# echo enabled=true ok=true echo plugin ready
bookclerk plugins echo ping --message hello
# pong: hello
```

## Distribution

**Native:** ship a directory (or archive) containing `plugin.toml` + binary for
the target OS/arch.

**Workerd / script:** ship `plugin.toml` + `modules/` (no per-OS binary required).
The operator’s Bookclerk install already includes `bookclerk-workerd` +
`bookclerk-jail`.

Users unpack under `plugins/` (or a `BOOKCLERK_PLUGIN_DIRS` root), **approve**
capabilities, then set `enabled = true` in `config.toml` (or
`bookclerk plugins enable`). No rebuild of Bookclerk is required when
`api_version` matches.

### First-party plugins (dual load via plugin host)

Audible, Libro.fm, Chirp, GraphicAudio, and Audiobookshelf ship as **external
plugins** under `crates/bookclerk-plugins/`. The host crate
`bookclerk-plugin-host` also registers the same adapters **in-process**
(`register_builtin_*` / `load_sources` / `load_integrations`) so `cargo run`
works without staging binaries. CLI/daemon call only those host helpers —
never store crates by name. Discovery skips an id that is already registered.

Guest binaries depend on **`bookclerk-plugin-sdk`** (+ their private store crate
for first-party). TypeScript workerd guests depend on **`@bookclerk/plugin-sdk`**.
Third-party authors should depend on the SDK only — not
`bookclerk-plugin-host`, `bookclerk-library`, or `bookclerk-source`.

CI builds and stages plugin artifacts with `cargo stage-plugins` (same as local
dev). Artifacts are **not** published to crates.io / GitHub Releases yet.

Locally:

```bash
cargo stage-plugins                    # build + stage to target/plugin-artifacts
cargo dev-daemon                       # stage + run bookclerkd
# or: BOOKCLERK_PLUGIN_ARTIFACTS=/tmp/bc-plugins cargo stage-plugins
```

For **crates.io naming**, release-asset conventions, and install-without-Rust
(planned `bookclerk plugins search|install` + dashboard browser), see
[plugin-registry.md](plugin-registry.md).
