fn main() {
    // Linux desktop is deferred until Tauri publishes a GTK4 / WebKitGTK 6
    // backend on crates.io. Shipping the current GTK3 graph would either pollute
    // the root OSV gate or force advisory ignores — neither is acceptable.
    // Use the authenticated web UI (bookclerkd) or a tray+browser companion.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        panic!(
            "bookclerk-desktop does not build for Linux yet: Tauri still resolves \
             unmaintained GTK3 / gtk-rs 0.18. Linux desktop is deferred until \
             Tauri GTK4 + WebKitGTK 6 lands on crates.io. Use the tray companion \
             / web UI via bookclerkd instead (see docs/gui-desktop-path.md)."
        );
    }

    #[cfg(any(windows, target_os = "macos"))]
    {
        tauri_build::build();
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // Host is not Win/macOS (e.g. Linux cross or other). Target already
        // rejected linux above; remaining hosts need a Win/macOS build env.
        panic!(
            "bookclerk-desktop Tauri build scripts require a Windows or macOS host \
             (see docs/gui-desktop-path.md)."
        );
    }
}
