//! Optionally start the in-process system tray when a graphical session is present.

use bookclerk_config::{
    graphical_session_available, operator_token_path, read_operator_token, Config,
};
use bookclerk_tray::TrayConfig;

/// Start the tray companion on a background OS thread when appropriate.
///
/// No-ops when:
/// - `[daemon].tray = false` / `BOOKCLERK_NO_TRAY` / `BOOKCLERK_DAEMON_TRAY=0`
/// - no windowing / session-bus environment (typical systemd/Docker hosts)
pub fn maybe_spawn_tray(config: &Config) {
    if !config.daemon.tray {
        tracing::debug!("daemon.tray disabled; not starting tray");
        return;
    }
    if !graphical_session_available() {
        tracing::debug!(
            "no graphical session (DISPLAY/WAYLAND_DISPLAY + session bus); \
             not starting tray"
        );
        return;
    }

    let base_url = TrayConfig::base_url(&config.daemon.listen);
    let auth_enabled = config.daemon.auth.enabled;
    let (operator_token, token_path) = if auth_enabled {
        match read_operator_token(config) {
            Ok(Some((token, _))) => (Some(token), Some(operator_token_path(config))),
            Ok(None) => (None, Some(operator_token_path(config))),
            Err(err) => {
                tracing::warn!(error = %err, "could not read operator token for tray");
                (None, Some(operator_token_path(config)))
            }
        }
    } else {
        (None, None)
    };

    let tray = TrayConfig {
        base_url: base_url.clone(),
        auth_enabled,
        operator_token,
        token_path,
    };

    // Open the browser once at startup (best-effort).
    if let Err(err) = tray.open_ui() {
        tracing::warn!(
            url = %tray.ui_url(),
            error = %err,
            "failed to open browser; use the tray menu to retry"
        );
    }

    let _handle = bookclerk_tray::spawn(tray);
    tracing::info!(%base_url, "started in-process system tray");
}
