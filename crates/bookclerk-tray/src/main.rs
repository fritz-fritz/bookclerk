//! Standalone tray companion: ensure `bookclerkd`, open the web UI in a browser.
//!
//! - Linux: StatusNotifierItem via `ksni` (no GTK/WebKit)
//! - Windows / macOS: `tray-icon` with default features disabled (no GTK)

mod daemon;
mod icon;

#[cfg(target_os = "linux")]
mod linux_tray;

#[cfg(any(windows, target_os = "macos"))]
mod native_tray;

use bookclerk_config::Config;
use daemon::DaemonHandle;

fn main() -> anyhow::Result<()> {
    let config = Config::load(None, None)?;
    let daemon = DaemonHandle::ensure(&config)?;
    daemon.open_ui()?;

    #[cfg(target_os = "linux")]
    {
        linux_tray::BookclerkTray::new(daemon, config).run()?;
    }

    #[cfg(any(windows, target_os = "macos"))]
    {
        native_tray::BookclerkTray::new(daemon, config).run()?;
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = config;
        eprintln!(
            "bookclerk-tray: no tray backend on this OS; browser opened at {}. Ctrl+C to exit.",
            daemon.base_url
        );
        loop {
            std::thread::park();
        }
    }

    Ok(())
}
