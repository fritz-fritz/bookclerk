# Windows AppContainer confinement notes

This page records enforcement details and measured assumptions for the
Windows spawn path. Green CI is necessary but not sufficient — each claim
below ties to code or an explicit limitation.

## Job Object ordering

Implemented in `bookclerk-sandbox` (`windows_launch.rs`), not rappct 0.13.3:

1. Default: `CREATE_SUSPENDED` → Job limits (`KILL_ON_JOB_CLOSE`, memory /
   active-process / optional CPU) → `AssignProcessToJobObject` → `ResumeThread`
   so no guest instruction runs before Job membership.
2. Optional: set `BOOKCLERK_AC_USE_JOB_LIST=1` to try
   `PROC_THREAD_ATTRIBUTE_JOB_LIST` first (with AppContainer caps + handle list).
   On hosts where that returns `ERROR_INVALID_HANDLE` (seen on GitHub
   `windows-latest`), CreateProcess retries the suspended path automatically.
3. On any failure after `CreateProcessW`, `TerminateProcess` the child and
   close process, thread, pipe, and Job handles.

Test hook: `BOOKCLERK_TEST_FAIL_JOB_ASSIGN=1` forces fail-closed teardown.

## Profile folder

`GetAppContainerFolderPath` is authoritative when the path is under Known Folder
LocalAppData `\Packages\`. Documented layout is
`%LOCALAPPDATA%\Packages\<moniker>\AC`; **measured on Windows CI** the API
returns `%LOCALAPPDATA%\Packages\<package-SID>`. Bookclerk then ensures the
`\AC` child exists and uses that as cwd / `LOCALAPPDATA` (`TEMP`/`TMP` →
`AC\Temp`). Fail closed on API failure or paths outside Packages — Bookclerk
does **not** synthesize a Packages path when the API fails.

## Cross-process ACL sync

Named mutex `Local\bookclerk-dacl-tx` (30s timeout) around every DACL RMW,
plus an in-process mutex. Revoke does not invalidate already-open handles.

## Interactive listen (Phase 0)

Guest `network = "listen"` remains available for plugins that serve inbound
workflows inside the guest (Audible LoginServer stays in the plugin). The
integration test `listen_poc_matrix_records_bind_results` records the matrix:

| ID | Caps (via `NetPolicy`) | Bind | Host client |
| --- | --- | --- | --- |
| A | `internetClient` (Outbound) | `127.0.0.1:0` | HTTP GET |
| B | + `internetClientServer` (Full) | `127.0.0.1:0` | HTTP GET |
| C | + `privateNetworkClientServer` (OutboundListen) | `0.0.0.0:0` | LAN IP |
| D | Outbound baseline | — | TCP `:443` out |
| E | Deny / no caps | `127.0.0.1:0` | — |
| F | Full + `CheckNetIsolation -a/-is` (SID) | `0.0.0.0:0` | HTTP GET via `127.0.0.1` (expect **ok**) |

Windows CI uploads a `listen-poc-matrix` artifact (`listen-poc.md` table +
`listen-poc.json`) from `listen_poc_matrix_records_bind_results`. Download it
from the Actions run — do not infer listen feasibility from a green check alone.

Read the table carefully: `bind_ok=true|false` is a measured probe report
(via `--listen-status` under the allowlisted write root). A missing report
(`no_report` / `no_probe_report`, or a failing test assertion) means the
harness/launch did not reach `TcpListener::bind` — **not** that AppContainer
denied listen.

### Expected results (Microsoft network isolation)

Capability SIDs and same-host loopback are **different knobs**:

- [`internetClientServer` / `privateNetworkClientServer`](https://learn.microsoft.com/en-us/windows/uwp/networking/networking-basics)
  authorize **remote** inbound (Internet / private LAN), not host↔AppContainer
  localhost IPC.
- Same-machine host→guest (or guest→host) on loopback requires a
  [`CheckNetIsolation LoopbackExempt`](https://learn.microsoft.com/en-us/windows/security/operating-system-security/network-security/windows-firewall/troubleshooting-uwp-firewall)
  (`-a` for client, `-is` for server inbound). Microsoft documents loopback as
  a **dev-only** escape hatch, not a shipping capability.
- Binding a socket can still succeed when inbound delivery will later be
  dropped — so `bind_ok=true` with `host_http=connect_failed` on A–C (and
  bind under E) is consistent with the docs on a single CI runner.

What CI can prove on a single runner:

| Cell | Expect / measured on GHA |
| --- | --- |
| D outbound TCP:443 | **succeed** (`internetClient`) |
| A–C host→guest | **fail connect** |
| F = Full + `CheckNetIsolation -a/-is` (SID), bind `0.0.0.0` | tool **active**, host→guest still **fail connect** (see below) |
| True remote inbound under B/C | **not measured** here (needs another machine) |

Row F is a **measurement-only** control. Bookclerk product code does **not**
invoke CheckNetIsolation (and must not ship a LoopbackExempt dependency).

On Windows CI, `-a -p=<SID>` and a live `-is -p=<SID>` both succeed, yet the
host still cannot complete TCP to the guest listener. So A–C are **not** fixed
by the documented UWP/IoT LoopbackExempt recipe for Bookclerk’s **unpackaged**
`CreateAppContainerProfile` guests. That still rules out “missing capability
SIDs” as the sole fix (B/F share Full caps); it means same-host host→guest
needs a host-owned bridge (or a different OS surface than CheckNetIsolation),
not a production LoopbackExempt dependency.

### Why Microsoft calls loopback exemption “dev only”

There is no AppContainer **capability SID** for “talk to localhost.” Capabilities
cover Internet / private-LAN remote traffic; same-host loopback is a separate
firewall isolation list (`AppContainerLoopback`). Microsoft’s own tooling help
text frames `LoopbackExempt` as easing **application development**, and older
network-isolation docs say loopback IPC between processes is not a supported
shipping pattern for Store/UWP-style containers.

Practical reasons there is no clean production option:

1. **Sandbox hole on purpose.** Exempting a package lets sandboxed code reach
   (or accept from) other processes on the machine over `127.0.0.1`, including
   high-value local services (SMB, developer tools, databases, admin APIs) that
   often assume “localhost = trusted.” That breaks the isolation model caps are
   meant to enforce.
2. **Admin / sticky config.** Changing the exemption list normally needs
   elevated rights (or Developer Mode shortcuts). Inbound `-is` must keep
   `CheckNetIsolation.exe` running for the listen window — not an app-private
   capability you declare in a manifest.
3. **No least-privilege grant.** The exemption is per AppContainer/package, not
   “only this port” or “only this peer.” Once exempt, the hole is broad.
4. **Not a Store capability.** Shipping products are expected to use remote
   networking caps or an out-of-container broker; relying on LoopbackExempt is
   treated like a debugger aid (VS enables it for debug sessions).

**Risk of using it in production:** any compromise or bug inside the guest
gains a path to host-local services that network isolation was blocking; a
host-local attacker (or another app) can more easily reach a guest listener
that was only intended for “same machine” UX. Edge’s own localhost flag
literally warns it can put the device at risk — same class of issue.

For Bookclerk: the host owns the browser TCP listener and proxies bytes to the
guest over IPC (`callback_proxy` + `callback_ipc` on `login.start`). The Windows
named pipe is ACLed to that guest’s Package SID (`GRGW`) with a Low mandatory
label (`S:(ML;;NW;;;LW)`); default pipe DACLs deny AppContainers and Medium-IL
objects fail the integrity check even when the DACL allows the SID. Product
code does **not** use CheckNetIsolation. See [plugins.md](plugins.md)
(Interactive listeners).

## Availability

| | Plugins | Media |
| --- | --- | --- |
| Process memory | 512 MiB | 2 GiB |
| Active processes | 8 | 64 |
| CPU rate | 80% hard cap | uncapped |
| Stderr proxy budget | 1 MiB | 16 MiB |
| data/tmp growth (plan) | 512 MiB each | n/a |
| RPC timeout | kill + quarantine | n/a (stdio job) |

Limits are best-effort Job Object + host policy, not a hard multi-tenant quota.

## AppContainer vs LPAC

Bookclerk uses regular AppContainer (capability SIDs + path ACLs + Job). Less
Privileged AppContainer (LPAC) ambient restrictions are not applied.

## Trust

Sandboxing ≠ publisher authentication. SHA-256 verifies artifact integrity, not
publisher identity. Install UX should surface registry/publisher/network (and
callback-transport if ever shipped) / signature before unattended install;
unsigned installs need explicit approval. See [plugins.md](plugins.md).

## Upstream

rappct 0.13.3 assigns the Job after the guest may already run and does not
resume suspended threads or reliably terminate on mid-launch failure. Bookclerk
bypasses `launch_in_container_with_io` for production launches and keeps rappct
for profile/SID/capability helpers only. File/track an upstream issue on
[cpjet64/rappct](https://github.com/cpjet64/rappct) describing:

1. `CreateProcess` then `AssignProcessToJobObject` race with a runnable primary thread
2. no `ResumeThread` when `suspended: true`
3. failure after create does not reliably `TerminateProcess`
