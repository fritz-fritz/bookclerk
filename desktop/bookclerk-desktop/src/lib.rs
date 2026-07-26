//! Tauri desktop shell: loads the shared React GUI and owns the system tray.
//!
//! Windows and macOS only. Linux is rejected in `build.rs` (and again here) until
//! Tauri ships GTK4 / WebKitGTK 6 without the advisory-pinned GTK3 graph.

#[cfg(any(windows, target_os = "macos"))]
mod app;

#[cfg(any(windows, target_os = "macos"))]
pub use app::run;

#[cfg(target_os = "linux")]
pub fn run() {
    panic!(
        "bookclerk-desktop does not build for Linux yet: Tauri still resolves \
         unmaintained GTK3 / gtk-rs 0.18. Use the tray companion / web UI via \
         bookclerkd instead (see docs/gui-desktop-path.md)."
    );
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn run() {
    panic!("bookclerk-desktop supports Windows and macOS only");
}
