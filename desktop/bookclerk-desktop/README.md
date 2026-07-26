# bookclerk-desktop

Tauri 2 shell + system tray for the shared React library UI.

Lives in a **nested Cargo workspace** (`desktop/`) so Windows/macOS packaging
does not pull GTK3/WebKitGTK into the root `Cargo.lock` (OSV gate). See
[`docs/gui-desktop-path.md`](../../docs/gui-desktop-path.md).

## Supported targets

| OS | Status |
| --- | --- |
| Windows | Supported (CI builds) |
| macOS | Supported (CI builds) |
| Linux | **Banned** in `build.rs` until Tauri GTK4 / WebKitGTK 6 on crates.io |

On Linux, use the authenticated web UI served by `bookclerkd`, or a separate
tray + system-browser companion.

## Local build (Windows / macOS)

```bash
cd ui && npm ci && npm run build
cargo build -p bookclerkd
cargo build --manifest-path desktop/Cargo.toml -p bookclerk-desktop
# or: cd desktop && cargo run -p bookclerk-desktop
```

Ensure `bookclerkd` is on `PATH` or beside the desktop binary so the shell can
spawn it when the configured listen address is unreachable.

## Upstream unblock (Linux)

Monitor:

- https://github.com/tauri-apps/tauri/issues/12563
- https://github.com/tauri-apps/tauri/pull/14684

When crates.io Tauri no longer pins GTK3 / `gtk-rs` 0.18, lift the `build.rs`
Linux panic, extend CI to Linux if desired, and re-evaluate folding this
workspace back into the root (only if the lockfile stays OSV-clean).
