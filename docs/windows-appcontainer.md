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

CI publishes lines prefixed `LISTEN_POC[…]` (also GitHub Job Summary). The
`listen_poc_matrix_records_bind_results` test is green on `windows-latest`.
**Callback transport remains unimplemented** until those summaries show that
guest bind/accept is insufficient for product needs — keep guest `listen`
mapping; CheckNetIsolation exemptions stay out of scope.

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
