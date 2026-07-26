# Desktop shell path evaluation

Decision record for native window + system tray around the shared React GUI
(`ui/` + `bookclerkd`). Companion to [gui.md](gui.md). Closes the loop on
tracking PR [#44](https://github.com/fritz-fritz/bookclerk/pull/44) by shipping
Windows/macOS via a nested workspace instead of merging GTK3 into mainline.

## What we need from a “desktop” shell

Bookclerk’s product GUI is already the **authenticated React UI** over HTTP.
A native shell only needs to:

1. Show that UI in a local window (optional if the system browser is acceptable)
2. Provide a **system tray / status-area** affordance (show/hide, scan, quit)
3. Ensure `bookclerkd` is running and can inject the operator token for
   auto-login
4. Package cleanly without dragging unmaintained or EOL Linux UI stacks into
   the default workspace / OSV gate

The shell must not host the API in TypeScript; Rust (`bookclerkd`) remains the
control plane.

## Constraints that rule options in or out

| Constraint | Implication |
| --- | --- |
| Shared React UI with remote daemon use | Prefer embedding or opening that UI; avoid rewriting screens in iced/egui/Avalonia |
| OSV / RUSTSEC gate; no advisory ignores on mainline | Cannot merge crates.io Tauri 2 into the **root** workspace while it pins GTK3 / `gtk-rs` 0.18 / old `glib` |
| Modern Linux defaults to Wayland | GTK3 + WebKitGTK 4.1 is a shrinking runtime target even when XWayland papers over some apps |
| Tray on Linux | StatusNotifierItem / DBus (`ksni`) is the modern path; classic libappindicator is GTK3-era and incompatible with GTK4 |
| No .NET | Avalonia / WinForms ports are out |

## Decision (current)

1. **Supported GUI everywhere:** authenticated web UI via `bookclerkd`.
2. **Ship Tauri on Windows and macOS now** from a **nested Cargo workspace**
   (`desktop/`, member `bookclerk-desktop`) that is **not** a root workspace
   member. Root `Cargo.lock` stays free of gtk / webkit2gtk / tauri; OSV scans
   the root tree with `--experimental-exclude desktop`.
3. **Linux native window stays deferred** until crates.io Tauri publishes GTK4
   + WebKitGTK 6 (+ ksni tray). `desktop/bookclerk-desktop/build.rs` panics on
   `CARGO_CFG_TARGET_OS=linux` with a clear message. Near-term Linux tray UX:
   ksni tray + system browser (separate companion), not GTK3 webview.
4. **Do not** add OSV `IgnoredVulns` for gtk in the root lockfile. **Do not**
   vendor unofficial GTK4 Tauri forks as the default path.
5. Tracking PR [#44](https://github.com/fritz-fritz/bookclerk/pull/44) preserved
   the shell while mainline stayed web-only; this nested-workspace approach is
   how that implementation lands without polluting the OSV gate.

## Upstream Tauri status (monitor for Linux unblock)

| Link | Role |
| --- | --- |
| [tauri#12563](https://github.com/tauri-apps/tauri/issues/12563) | Canonical “upgrade tauri to gtk4-rs” issue |
| [tauri#14684](https://github.com/tauri-apps/tauri/pull/14684) | Integration PR: GTK4 + WebKitGTK 6.0 (open, not merge-ready) |
| [tao#1258](https://github.com/tauri-apps/tao/pull/1258) | Fresher GTK4 tao port |
| wry / muda / tray-icon + **ksni** | Required for tray once libappindicator drops away |

## Options considered

### A. Nested Win/macOS Tauri workspace (adopted)

`desktop/Cargo.toml` is a virtual workspace with `bookclerk-desktop` only.
Path-depends on `../../crates/bookclerk-config`. CI builds on `macos-latest`
and `windows-latest`. Nested `desktop/Cargo.lock` may still *resolve* Linux
GTK packages for cargo’s multi-target graph — that is OK there; OSV excludes
`desktop/`. Root lockfile must stay clean.

### B. Wait for crates.io Tauri on GTK4 / WebKitGTK 6 (Linux destination)

Keep monitoring upstream. When the advisory-pinned GTK3 graph is gone, lift
the Linux `build.rs` ban and optionally fold packaging back into the root
workspace **only if** OSV stays green without ignores.

### C. Accept GTK3 advisories in root (idento-style) — rejected

Audit ignore does not fix Wayland/modern distro runtime and reintroduces OSV
ignores we refuse for mainline.

### D. Vendor unofficial GTK4 Tauri forks — rejected as default

Patch ownership and supply-chain cost are wrong for a thin shell.

### E. Tray companion + system browser — near-term Linux alternative

Small Rust binary via `ksni` (or OS equivalents), spawn/attach `bookclerkd`,
open the operator UI in the system browser. No embedded webview → no GTK3 pin.

### F. Electron / CEF / Qt / iced / egui rewrite — rejected

Heavyweight or duplicates the React investment; Dioxus desktop still bottoms
out on wry/WebKit on Linux today.

## Layout

```text
desktop/                          # nested workspace (not root member)
  Cargo.toml
  Cargo.lock                      # may list tauri (+ gtk for Linux target res.)
  bookclerk-desktop/
    build.rs                      # panics on linux
    tauri.conf.json               # frontendDist ../../ui/dist
    src/
ui/                               # shared React UI (+ optional @tauri-apps/api)
crates/bookclerk-config/          # path dep from desktop package
Cargo.toml / Cargo.lock           # root — no gtk / tauri / webkit2gtk
```

## Revisit triggers

- Tauri (or tao+wry) publishes GTK4/WebKitGTK 6 on crates.io and nested (or
  root) lockfile is OSV-clean without ignores
- [tauri#12563](https://github.com/tauri-apps/tauri/issues/12563) closed with a
  release, or #14684 abandoned without a successor (then re-evaluate E)
- Desire to collapse `desktop/` back into the root workspace once Linux is safe
