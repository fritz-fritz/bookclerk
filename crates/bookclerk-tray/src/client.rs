//! Talk to the local `bookclerkd` HTTP API from the tray thread.

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Configuration for the in-process tray (no child `bookclerkd` process).
#[derive(Debug, Clone)]
pub struct TrayConfig {
    /// Daemon HTTP base URL (e.g. `http://127.0.0.1:8787`).
    pub base_url: String,
    /// When true, operator-token auth is required for tray actions.
    pub auth_enabled: bool,
    /// Operator bearer token used for authenticated daemon calls.
    pub operator_token: Option<String>,
}

/// Shared tray config so the daemon can refresh `base_url` after listen rebinds.
pub type SharedTrayConfig = Arc<Mutex<TrayConfig>>;

/// How long a copied operator token stays on the clipboard before it is cleared.
///
/// Long enough to paste into the login field; short enough that a forgotten
/// copy does not linger. After this TTL the tray looks up this exact token and
/// removes it if it is still present.
const OPERATOR_TOKEN_CLIPBOARD_TTL: Duration = Duration::from_secs(60);

impl TrayConfig {
    /// Builds an HTTP base URL from a daemon listen address.
    ///
    /// # Arguments
    ///
    /// * `listen` - `host:port` or an absolute `http(s)://` URL (trailing `/` stripped).
    ///
    /// # Returns
    ///
    /// Absolute base URL with an `http://` scheme when `listen` was host-only.
    #[must_use]
    pub fn base_url(listen: &str) -> String {
        let listen = listen.trim().trim_end_matches('/');
        if listen.starts_with("http://") || listen.starts_with("https://") {
            listen.to_string()
        } else {
            format!("http://{listen}")
        }
    }

    /// Open the UI via a loopback handoff URL that sets the session cookie.
    ///
    /// The durable operator token is never placed in the URL. [`Self::open_ui`]
    /// POSTs `/api/auth/tray-handoff/prepare` with Bearer first, then opens this
    /// GET so Linux `xdg-open` does not need a fragment (which it often strips).
    #[must_use]
    pub fn ui_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if self.auth_enabled
            && self
                .operator_token
                .as_deref()
                .is_some_and(|t| !t.is_empty())
        {
            format!("{base}/api/auth/tray-handoff")
        } else {
            format!("{base}/")
        }
    }

    /// Opens the daemon web UI in the default browser.
    ///
    /// # Errors
    ///
    /// Returns an error when prepare fails or the OS cannot launch a browser
    /// for [`Self::ui_url`].
    pub fn open_ui(&self) -> anyhow::Result<()> {
        if self.auth_enabled {
            self.prepare_tray_handoff()?;
        }
        open::that(self.ui_url())?;
        Ok(())
    }

    /// Mint a short-lived loopback handoff ticket (Bearer; no secret in the GET URL).
    fn prepare_tray_handoff(&self) -> anyhow::Result<()> {
        let url = format!(
            "{}/api/auth/tray-handoff/prepare",
            self.base_url.trim_end_matches('/')
        );
        let mut req = ureq::post(&url)
            .config()
            .timeout_global(Some(Duration::from_secs(10)))
            .build();
        if let Some(token) = self.operator_token.as_deref().filter(|t| !t.is_empty()) {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req.send_empty()?;
        Ok(())
    }

    /// POSTs an authenticated library scan request to the daemon.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the daemon rejects it.
    pub fn trigger_scan(&self) -> anyhow::Result<()> {
        let url = format!("{}/api/library/scan", self.base_url.trim_end_matches('/'));
        let mut req = ureq::post(&url)
            .header("Content-Type", "application/json")
            .config()
            .timeout_global(Some(Duration::from_secs(10)))
            .build();
        if self.auth_enabled {
            if let Some(token) = self.operator_token.as_deref() {
                req = req.header("Authorization", format!("Bearer {token}"));
            }
        }
        req.send("{}")?;
        Ok(())
    }

    /// Copy the operator token to the system clipboard (never prints the value).
    pub fn copy_operator_token(&self) {
        if !self.auth_enabled {
            tracing::info!("operator auth is disabled");
            return;
        }
        match self.operator_token.as_deref() {
            Some(token) if !token.is_empty() => {
                let token = token.to_owned();
                if std::thread::Builder::new()
                    .name("bookclerk-clipboard".into())
                    .spawn(move || persist_operator_token(token))
                    .is_err()
                {
                    tracing::warn!("clipboard unavailable — run `bookclerk daemon token`");
                }
            }
            _ => tracing::warn!("no operator token available"),
        }
    }

    /// Updates the configured daemon listen address used to derive [`Self::base_url`].
    ///
    /// # Arguments
    ///
    /// * `listen` - Daemon listen address (`host:port` or URL).
    pub fn set_listen(&mut self, listen: &str) {
        self.base_url = Self::base_url(listen);
    }

    /// Overrides the daemon HTTP base URL used for subsequent calls.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Absolute HTTP base URL for the daemon.
    pub fn set_base_url(&mut self, base_url: impl Into<String>) {
        self.base_url = base_url.into();
    }
}

/// Writes `token` to the clipboard, keeps the selection alive for
/// [`OPERATOR_TOKEN_CLIPBOARD_TTL`], then removes that exact value if present.
///
/// Holding the handle for the TTL avoids arboard's Linux warning (and missed
/// pastes) when `Clipboard` is dropped immediately after `set_text`. Cleanup
/// targets the secret itself: overlapping copies and later user pastes do not
/// need a generation counter.
fn persist_operator_token(token: String) {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => clipboard,
        Err(_) => {
            tracing::warn!("clipboard unavailable — run `bookclerk daemon token`");
            return;
        }
    };
    if clipboard.set_text(token.clone()).is_err() {
        tracing::warn!("clipboard unavailable — run `bookclerk daemon token`");
        return;
    }
    tracing::info!(
        ttl_secs = OPERATOR_TOKEN_CLIPBOARD_TTL.as_secs(),
        "operator token copied to clipboard"
    );
    std::thread::sleep(OPERATOR_TOKEN_CLIPBOARD_TTL);
    match remove_secret_from_clipboard(&mut clipboard, &token) {
        ClipboardSecretRemoval::Removed => {
            tracing::debug!("operator token cleared from clipboard");
        }
        ClipboardSecretRemoval::NotPresent => {}
        ClipboardSecretRemoval::ClearFailed => {
            tracing::debug!("could not clear operator token from clipboard");
        }
    }
}

/// Whether [`remove_secret_from_clipboard`] found and deleted the secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardSecretRemoval {
    /// Clipboard text matched the secret and was cleared.
    Removed,
    /// Clipboard was empty, unreadable, or held a different value.
    NotPresent,
    /// Clipboard still held the secret but `clear` failed.
    ClearFailed,
}

/// Removes `secret` from `clipboard` only when the current text is exactly that value.
fn remove_secret_from_clipboard(
    clipboard: &mut arboard::Clipboard,
    secret: &str,
) -> ClipboardSecretRemoval {
    match clipboard.get_text() {
        Ok(current) if clipboard_text_is_secret(&current, secret) => {
            if clipboard.clear().is_ok() {
                ClipboardSecretRemoval::Removed
            } else {
                ClipboardSecretRemoval::ClearFailed
            }
        }
        _ => ClipboardSecretRemoval::NotPresent,
    }
}

/// True when `current` is exactly the secret the tray placed on the clipboard.
fn clipboard_text_is_secret(current: &str, secret: &str) -> bool {
    current == secret
}

#[cfg(test)]
mod tests {
    use super::{clipboard_text_is_secret, TrayConfig};

    #[test]
    fn base_url_normalizes() {
        assert_eq!(
            TrayConfig::base_url("127.0.0.1:8787"),
            "http://127.0.0.1:8787"
        );
        assert_eq!(
            TrayConfig::base_url("http://127.0.0.1:8787/"),
            "http://127.0.0.1:8787"
        );
    }

    #[test]
    fn ui_url_has_no_query_token() {
        let cfg = TrayConfig {
            base_url: "http://127.0.0.1:8787".into(),
            auth_enabled: true,
            operator_token: Some("abc".into()),
        };
        assert_eq!(cfg.ui_url(), "http://127.0.0.1:8787/api/auth/tray-handoff");
        assert!(!cfg.ui_url().contains("token="));
    }

    #[test]
    fn ui_url_without_auth_is_root() {
        let cfg = TrayConfig {
            base_url: "http://127.0.0.1:8787".into(),
            auth_enabled: false,
            operator_token: Some("abc".into()),
        };
        assert_eq!(cfg.ui_url(), "http://127.0.0.1:8787/");
    }

    #[test]
    fn set_listen_updates_base_url() {
        let mut cfg = TrayConfig {
            base_url: "http://127.0.0.1:8787".into(),
            auth_enabled: false,
            operator_token: None,
        };
        cfg.set_listen("127.0.0.1:9999");
        assert_eq!(cfg.base_url, "http://127.0.0.1:9999");
    }

    #[test]
    fn clipboard_removal_targets_the_exact_secret() {
        assert!(clipboard_text_is_secret("tok", "tok"));
        assert!(!clipboard_text_is_secret("other", "tok"));
        assert!(!clipboard_text_is_secret("", "tok"));
    }
}
