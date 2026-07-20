//! OAuth login orchestration (QR + callback server + response-url).

use std::net::SocketAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AudibleError, Result};
use crate::qr::{render_login_qr, QrRenderMode};

/// Options for `libation auth login`.
#[derive(Debug, Clone)]
pub struct AuthLoginOptions {
    /// Marketplace code (`us`, `uk`, `de`, …).
    pub marketplace: String,
    /// Optional account label.
    pub label: Option<String>,
    /// Bind address for the embedded callback server.
    pub callback_bind: SocketAddr,
    /// Fully non-interactive: caller supplies the Amazon redirect URL.
    pub response_url: Option<String>,
    /// Print QR for SSH / headless use.
    pub show_qr: bool,
    pub qr_mode: QrRenderMode,
    /// Where to store auth material (Libation files dir).
    pub files_dir: PathBuf,
}

impl Default for AuthLoginOptions {
    fn default() -> Self {
        Self {
            marketplace: "us".into(),
            label: None,
            callback_bind: "127.0.0.1:0".parse().expect("valid socket addr"),
            response_url: None,
            show_qr: true,
            qr_mode: QrRenderMode::Unicode,
            files_dir: PathBuf::from("LibationFiles"),
        }
    }
}

/// Progress events emitted during login (for CLI / TUI).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoginProgress {
    /// Login URL ready — show QR / print URL.
    LoginUrl { url: String, qr: Option<String> },
    /// Callback server listening.
    CallbackListening { addr: SocketAddr },
    /// Waiting for the user / browser.
    WaitingForCallback,
    /// Tokens acquired.
    Completed { account_id: String },
}

/// Result of a successful auth session (scaffold placeholder).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub account_id: String,
    pub marketplace: String,
    pub label: Option<String>,
}

/// Begin the login flow.
///
/// Phase 1 scaffold: when `--response-url` is absent, prints a placeholder URL
/// and QR so CLI wiring can be exercised. Real audible-rs login is enabled via
/// the `audible-rs` feature in a follow-up change.
pub async fn begin_login(
    opts: AuthLoginOptions,
    mut on_progress: impl FnMut(LoginProgress),
) -> Result<AuthSession> {
    if let Some(response_url) = opts.response_url.clone() {
        tracing::info!("completing login from --response-url");
        return complete_from_response_url(opts, &response_url).await;
    }

    // Placeholder URL documents the intended UX until audible-rs wiring lands.
    let placeholder = format!(
        "https://www.amazon.com/ap/signin?openid.assoc_handle=amzn_audible_{}_us\
         &marketplace={}&libation=pending",
        opts.marketplace, opts.marketplace
    );

    let qr = if opts.show_qr {
        Some(render_login_qr(&placeholder, opts.qr_mode)?)
    } else {
        None
    };

    on_progress(LoginProgress::LoginUrl {
        url: placeholder.clone(),
        qr,
    });
    on_progress(LoginProgress::CallbackListening {
        addr: opts.callback_bind,
    });
    on_progress(LoginProgress::WaitingForCallback);

    Err(AudibleError::Auth(
        "interactive login is not fully wired yet; use --response-url once you have the \
         Amazon redirect, or enable the audible-rs feature (follow-up)"
            .into(),
    ))
}

async fn complete_from_response_url(
    opts: AuthLoginOptions,
    response_url: &str,
) -> Result<AuthSession> {
    if response_url.trim().is_empty() {
        return Err(AudibleError::Auth("empty --response-url".into()));
    }

    // Scaffold: accept the URL shape and record a stub session id.
    // Real token exchange happens when the audible-rs feature is enabled.
    let account_id = format!("pending-{}", opts.marketplace);
    tracing::warn!(
        %account_id,
        files_dir = %opts.files_dir.display(),
        "auth login --response-url accepted (stub); audible-rs exchange pending"
    );

    Ok(AuthSession {
        account_id,
        marketplace: opts.marketplace,
        label: opts.label,
    })
}
