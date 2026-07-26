# Desktop shell path evaluation

Decision record for native window + system tray around the shared React GUI
(`ui/` + `bookclerkd`). Companion to [gui.md](gui.md) and tracking PR
[#44](https://github.com/fritz-fritz/bookclerk/pull/44).

## What we need from a “desktop” shell

Bookclerk’s product GUI is already the **authenticated React UI** over HTTP.
A native shell only needs to:

1. Show that UI in a local window (optional if the system browser is acceptable)
2. Provide a **system tray / status-area** affordance (show/hide, scan, quit)
3. Ensure `bookclerkd` is running and can inject the operator token for
   auto-login
4. Ship without advisory-pinned or unscanned UI stacks in any lockfile the
   repo’s OSV gate is responsible for

The shell must not host the API in TypeScript; Rust (`bookclerkd`) remains the
control plane.

## Constraints

| Constraint | Implication |
| --- | --- |
| OSV / RUSTSEC gate; no advisory ignores | Stock crates.io Tauri 2 still resolves GTK3 / `gtk-rs` 0.18 / old `glib` into `Cargo.lock` |
| Cargo lockfiles are multi-target | Platform-gating builds (Windows/macOS only) does **not** omit Linux target packages from the lockfile |
| Scanning discipline | Excluding a packaging lockfile from OSV, or adding `IgnoredVulns`, is still “don’t fail on known advisories” — **not acceptable** for a product binary we ship |
| Modern Linux / Wayland | GTK3 + WebKitGTK 4.1 is a shrinking runtime even aside from advisories |
| No .NET | Avalonia / WinForms ports are out |

## Decision (current)

1. **Supported GUI:** authenticated web UI via `bookclerkd` on all platforms.
2. **Do not ship** `bookclerk-desktop` (Tauri) on mainline — including
   Windows/macOS-only packaging — until a lockfile that includes the shell is
   OSV-clean **without** ignores and **without** excluding that lockfile from
   the scan.
3. Preserve the implementation on tracking PR
   [#44](https://github.com/fritz-fritz/bookclerk/pull/44) for a future bump
   when crates.io Tauri (tao/wry/muda/tray-icon) publishes GTK4 + WebKitGTK 6
   (+ ksni tray) without the advisory-pinned GTK3 graph.
4. Near-term Linux tray UX, if needed before that: ksni tray + system browser
   (no embedded webview) — separate from Tauri.
5. **Do not** vendor unofficial GTK4 Tauri forks as the default path.

## Rejected: Win/macOS-only nested workspace + OSV exclude

Evaluated in [#46](https://github.com/fritz-fritz/bookclerk/pull/46): move
`bookclerk-desktop` to a nested `desktop/` Cargo workspace so the **root**
`Cargo.lock` stays free of gtk/tauri, CI builds only on macOS/Windows, and OSV
uses `--experimental-exclude desktop`.

**Why that fails the bar:** `desktop/Cargo.lock` still resolves the full GTK3
stack (Cargo records all targets). Scanning that lockfile alone reports the
same RUSTSEC/GTK advisories (atk/gdk/glib/gtk/… plus related unmaintained
crates). Excluding `desktop/` from OSV is functionally the same as ignoring
those findings for a binary we would ship. Moving the workspace does not create
a vulnerability-free Win/macOS graph; it only hides the Linux target packages
from the gate.

Until stock Tauri’s resolved graph is clean under a full OSV scan, platform
gating and lockfile isolation are **not** mergeable shortcuts.

## Upstream Tauri status (unblock signal)

| Link | Role |
| --- | --- |
| [tauri#12563](https://github.com/tauri-apps/tauri/issues/12563) | Canonical “upgrade tauri to gtk4-rs” issue |
| [tauri#14684](https://github.com/tauri-apps/tauri/pull/14684) | Integration PR: GTK4 + WebKitGTK 6.0 (open, not merge-ready) |
| [tao#1258](https://github.com/tauri-apps/tao/pull/1258) | Fresher GTK4 tao port |
| wry / muda / tray-icon + **ksni** | Required for tray once libappindicator drops away |

**Unblock checklist:** tao/wry/muda/tray-icon (ksni) released → tauri release on
that graph → `cargo update` → OSV green on the desktop (or root) lockfile with
**no** ignores and **no** path excludes for that lockfile → smoke tray +
Wayland + X11 (when Linux is enabled).

## Other options (unchanged)

| Option | Verdict |
| --- | --- |
| Accept GTK3 advisories (idento-style ignores) | **Rejected** for mainline |
| Vendor unofficial GTK4 Tauri / tao / wry forks | **Rejected** as default (patch ownership) |
| Tray companion + system browser (`ksni`) | **OK near-term** if Linux tray is needed before upstream lands |
| Electron / CEF / Qt / iced / egui rewrite | **Rejected** for this product shape |

## Revisit triggers

- Tauri (or tao+wry) publishes GTK4/WebKitGTK 6 on crates.io and OSV is clean
  without ignores or scan excludes
- [tauri#12563](https://github.com/tauri-apps/tauri/issues/12563) closed with a
  release, or #14684 abandoned without a successor (then re-evaluate tray+browser
  vs other embedders)
