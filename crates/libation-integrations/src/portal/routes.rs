//! Axum routes for the connect portal.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use libation_config::Config;
use libation_library::LibraryStore;
use libation_source::{ContentSource, LoginOptions, SourceKind};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::brands::{integration_brand, source_brand};
use super::html::{credential_login_brands, landing_page};
use crate::registry::IntegrationRegistry;
use crate::tickets::{
    identity_from_session, mint_claim_ticket, normalize_portal_base, redeem_ticket_to_session,
    session_for_identity,
};
use crate::types::ExternalUser;

const SESSION_COOKIE: &str = "libation_portal_session";

/// Shared state for portal handlers.
#[derive(Clone)]
pub struct PortalState {
    pub config: Arc<RwLock<Config>>,
    pub library: LibraryStore,
    pub integrations: IntegrationRegistry,
    pub files_dir: PathBuf,
    pub sources: Vec<Arc<dyn ContentSource>>,
}

pub fn portal_router(state: PortalState) -> Router {
    Router::new()
        .route("/", get(landing))
        .route("/api/redeem", post(redeem))
        .route("/api/login/integration", post(login_integration))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/sources", get(sources))
        .route("/api/sources/{id}/login", post(source_password_login))
        .route("/api/sources/{id}/oauth/start", post(source_oauth_start))
        // Legacy aliases kept for older portal clients / smoke scripts.
        .route("/api/libro/login", post(libro_login_legacy))
        .route("/api/audible/start", post(audible_start))
        .route("/api/connections", get(connections))
        .route(
            "/api/connections/{account_id}/revoke",
            post(revoke_connection),
        )
        .with_state(state)
}

async fn landing(State(state): State<PortalState>) -> Html<String> {
    let cfg = state.config.read().await;
    let base = normalize_portal_base(&cfg.integrations.portal_base_path);
    let mut providers = Vec::new();
    if cfg.integrations.audiobookshelf.allow_credential_login
        && cfg.integrations.audiobookshelf.enabled
        && state.integrations.get("audiobookshelf").is_some()
    {
        providers.push("audiobookshelf".into());
    }
    drop(cfg);
    let brands = credential_login_brands(&providers);
    Html(landing_page(&base, &brands))
}

#[derive(Debug, Deserialize)]
struct RedeemBody {
    ticket: String,
}

async fn redeem(
    State(state): State<PortalState>,
    Json(body): Json<RedeemBody>,
) -> Result<Response, PortalError> {
    let cfg = state.config.read().await;
    let (session, identity) =
        redeem_ticket_to_session(&state.library, &cfg.integrations, body.ticket.trim())?;
    drop(cfg);
    info!(
        identity_id = identity.id,
        provider = %identity.provider,
        "claim ticket redeemed"
    );
    Ok(session_response(session, &state).await)
}

#[derive(Debug, Deserialize)]
struct IntegrationLoginBody {
    provider: String,
    username: String,
    password: String,
}

async fn login_integration(
    State(state): State<PortalState>,
    Json(body): Json<IntegrationLoginBody>,
) -> Result<Response, PortalError> {
    let integration = state
        .integrations
        .get(&body.provider)
        .ok_or_else(|| PortalError::bad("unknown integration provider"))?;
    let user = integration
        .authenticate_user(body.username.trim(), &body.password)
        .await
        .map_err(|e| PortalError::bad(e.to_string()))?;
    let identity = state.library.upsert_portal_identity(
        &user.provider,
        &user.external_user_id,
        user.display_name.as_deref(),
    )?;
    let cfg = state.config.read().await;
    let session = session_for_identity(&state.library, &cfg.integrations, &identity)?;
    drop(cfg);
    Ok(session_response(session, &state).await)
}

async fn logout(State(state): State<PortalState>) -> Response {
    let cfg = state.config.read().await;
    let base = normalize_portal_base(&cfg.integrations.portal_base_path);
    let cookie = format!("{SESSION_COOKIE}=; Path={base}; HttpOnly; SameSite=Lax; Max-Age=0");
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

async fn me(
    State(state): State<PortalState>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, PortalError> {
    let identity = require_identity(&state, &headers).await?;
    Ok(Json(MeResponse {
        provider: identity.provider,
        external_user_id: identity.external_user_id,
        label: identity.label,
    }))
}

#[derive(Debug, Serialize)]
struct MeResponse {
    provider: String,
    external_user_id: String,
    label: Option<String>,
}

async fn sources(State(state): State<PortalState>) -> Json<SourcesResponse> {
    let mut list = Vec::new();
    for s in &state.sources {
        let kind = s.kind();
        let brand = source_brand(kind);
        list.push(SourceInfo {
            id: kind.as_str().into(),
            name: kind.display_name().into(),
            auth: kind.portal_auth_mode().into(),
            brand: BrandInfo {
                bg: brand.bg.into(),
                fg: brand.fg.into(),
                accent: brand.accent.into(),
                logo: brand.logo_href().into(),
            },
        });
    }
    Json(SourcesResponse { sources: list })
}

#[derive(Debug, Serialize)]
struct SourcesResponse {
    sources: Vec<SourceInfo>,
}

#[derive(Debug, Serialize)]
struct SourceInfo {
    id: String,
    name: String,
    auth: String,
    brand: BrandInfo,
}

#[derive(Debug, Serialize)]
struct BrandInfo {
    bg: String,
    fg: String,
    accent: String,
    logo: String,
}

#[derive(Debug, Deserialize)]
struct PasswordLoginBody {
    email: String,
    password: String,
}

async fn source_password_login(
    State(state): State<PortalState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PasswordLoginBody>,
) -> Result<Json<serde_json::Value>, PortalError> {
    let identity = require_identity(&state, &headers).await?;
    let kind = SourceKind::parse(&id).ok_or_else(|| PortalError::bad("unknown source"))?;
    if kind.portal_auth_mode() != "password" {
        return Err(PortalError::bad(
            "this source uses OAuth; call /oauth/start instead",
        ));
    }
    let source = state
        .sources
        .iter()
        .find(|s| s.kind() == kind)
        .cloned()
        .ok_or_else(|| {
            PortalError::bad(format!("{} source not registered", kind.display_name()))
        })?;

    let account = source
        .login(
            &state.files_dir,
            LoginOptions {
                marketplace: "us".into(),
                label: None,
                email: Some(body.email.trim().to_string()),
                password: Some(body.password),
                force: true,
            },
        )
        .await
        .map_err(|e| PortalError::bad(e.to_string()))?;

    state.library.upsert_account_with_source(
        &account.account_id,
        &account.marketplace,
        account.label.as_deref(),
        true,
        kind.as_str(),
    )?;
    state.library.mark_connection_active(&account.account_id)?;
    state
        .library
        .link_account(identity.id, &account.account_id, kind.as_str())?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "account_id": account.account_id,
        "source": kind.as_str(),
    })))
}

async fn source_oauth_start(
    State(state): State<PortalState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, PortalError> {
    let identity = require_identity(&state, &headers).await?;
    let kind = SourceKind::parse(&id).ok_or_else(|| PortalError::bad("unknown source"))?;
    if kind != SourceKind::Audible {
        return Err(PortalError::bad(
            "OAuth start is only implemented for Audible",
        ));
    }
    if !state
        .sources
        .iter()
        .any(|s| s.kind() == SourceKind::Audible)
    {
        return Err(PortalError::bad("Audible source not registered"));
    }
    let url = start_audible_login_session(&state, identity.id).await?;
    Ok(Json(serde_json::json!({ "url": url })))
}

async fn libro_login_legacy(
    State(state): State<PortalState>,
    headers: HeaderMap,
    Json(body): Json<PasswordLoginBody>,
) -> Result<Json<serde_json::Value>, PortalError> {
    source_password_login(State(state), headers, Path("libro".into()), Json(body)).await
}

async fn audible_start(
    State(state): State<PortalState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, PortalError> {
    source_oauth_start(State(state), headers, Path("audible".into())).await
}

/// Start Audible LoginServer bound for reverse-proxy use; return the browser URL.
///
/// Uses `0.0.0.0:0` so the daemon accepts remote callbacks when published behind
/// a reverse proxy. The printed URL may still show a container-local host; set
/// `integrations.public_origin` and rewrite as needed at the proxy layer.
async fn start_audible_login_session(
    state: &PortalState,
    identity_id: i64,
) -> Result<String, PortalError> {
    use libation_audible::{begin_login, AuthLoginOptions, LoginProgress};
    use tokio::sync::mpsc;

    let files_dir = state.files_dir.clone();
    let password_file = state.config.read().await.auth.password_file.clone();
    let allow_plaintext = state.config.read().await.auth.allow_plaintext;
    let library = state.library.clone();
    let (url_tx, mut url_rx) = mpsc::channel::<String>(1);

    tokio::spawn(async move {
        let opts = AuthLoginOptions {
            files_dir,
            password_file,
            allow_plaintext,
            show_qr: false,
            callback_bind: "0.0.0.0:0".parse().expect("bind"),
            ..Default::default()
        };
        let url_tx2 = url_tx.clone();
        let result = begin_login(opts, move |progress| {
            if let LoginProgress::LoginUrl { url, .. } = &progress {
                let _ = url_tx2.try_send(url.clone());
            }
        })
        .await;
        match result {
            Ok(session) => {
                let _ = library.upsert_account_with_source(
                    &session.account_id,
                    &session.marketplace,
                    session.label.as_deref(),
                    true,
                    "audible",
                );
                let _ = library.mark_connection_active(&session.account_id);
                let _ = library.link_account(identity_id, &session.account_id, "audible");
                info!(account = %session.account_id, "portal Audible login completed");
            }
            Err(err) => warn!(%err, "portal Audible login failed"),
        }
    });

    let url = tokio::time::timeout(std::time::Duration::from_secs(5), url_rx.recv())
        .await
        .map_err(|_| PortalError::bad("timed out waiting for Audible login URL"))?
        .ok_or_else(|| PortalError::bad("Audible login URL channel closed"))?;
    Ok(url)
}

async fn connections(
    State(state): State<PortalState>,
    headers: HeaderMap,
) -> Result<Json<ConnectionsResponse>, PortalError> {
    let identity = require_identity(&state, &headers).await?;
    let links = state.library.list_account_links(identity.id)?;
    let mut connections = Vec::new();
    for link in links {
        let acct = state.library.get_account(&link.account_id)?;
        let brand = SourceKind::parse(&link.source)
            .map(source_brand)
            .or_else(|| integration_brand(&link.source));
        connections.push(ConnectionInfo {
            account_id: link.account_id,
            source: link.source,
            label: acct.as_ref().and_then(|a| a.label.clone()),
            connection_status: acct
                .map(|a| a.connection_status)
                .unwrap_or_else(|| "active".into()),
            brand: brand.map(|b| BrandInfo {
                bg: b.bg.into(),
                fg: b.fg.into(),
                accent: b.accent.into(),
                logo: b.logo_href().into(),
            }),
        });
    }
    Ok(Json(ConnectionsResponse { connections }))
}

#[derive(Debug, Serialize)]
struct ConnectionsResponse {
    connections: Vec<ConnectionInfo>,
}

#[derive(Debug, Serialize)]
struct ConnectionInfo {
    account_id: String,
    source: String,
    label: Option<String>,
    connection_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    brand: Option<BrandInfo>,
}

async fn revoke_connection(
    State(state): State<PortalState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, PortalError> {
    let identity = require_identity(&state, &headers).await?;
    let links = state.library.list_account_links(identity.id)?;
    if !links.iter().any(|l| l.account_id == account_id) {
        return Err(PortalError::bad("account not linked to this identity"));
    }
    revoke_auth_files(&state.files_dir, &account_id);
    state.library.revoke_credentials(&account_id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

fn revoke_auth_files(files_dir: &std::path::Path, account_id: &str) {
    let accounts = files_dir.join("Accounts");
    if let Ok(entries) = std::fs::read_dir(&accounts) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Matches Audible `.auth`, Libro `.libro.auth`, Chirp `.chirp.auth`, GA `.ga.auth`.
            if name.starts_with(account_id) && name.ends_with(".auth") {
                match std::fs::remove_file(entry.path()) {
                    Ok(()) => info!(path = %entry.path().display(), "removed auth file on revoke"),
                    Err(err) => {
                        warn!(path = %entry.path().display(), %err, "failed to remove auth file")
                    }
                }
            }
        }
    }
}

async fn require_identity(
    state: &PortalState,
    headers: &HeaderMap,
) -> Result<libation_library::PortalIdentity, PortalError> {
    let raw = cookie_value(headers, SESSION_COOKIE)
        .ok_or_else(|| PortalError::unauthorized("not signed in"))?;
    identity_from_session(&state.library, &raw)?
        .ok_or_else(|| PortalError::unauthorized("session expired"))
}

async fn session_response(session: String, state: &PortalState) -> Response {
    let cfg = state.config.read().await;
    let base = normalize_portal_base(&cfg.integrations.portal_base_path);
    let max_age = cfg.integrations.portal_session_ttl_hours * 3600;
    let cookie = format!(
        "{SESSION_COOKIE}={session}; Path={base}; HttpOnly; SameSite=Lax; Max-Age={max_age}"
    );
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix(name) {
            if let Some(v) = rest.strip_prefix('=') {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Mint a claim ticket for an external user (daemon watcher / CLI).
pub fn mint_for_external_user(
    library: &LibraryStore,
    config: &Config,
    user: &ExternalUser,
    created_by: &str,
) -> crate::Result<crate::tickets::MintedClaimTicket> {
    let minted = mint_claim_ticket(library, &config.integrations, user, created_by)?;
    if let Some(url) = crate::tickets::ticket_portal_url(&config.integrations, &minted.token) {
        info!(%url, identity = minted.identity.id, "minted claim ticket");
    } else {
        info!(
            token = %minted.token,
            identity = minted.identity.id,
            "minted claim ticket (set integrations.public_origin to log a URL)"
        );
    }
    Ok(minted)
}

#[derive(Debug)]
struct PortalError {
    status: StatusCode,
    message: String,
}

impl PortalError {
    fn bad(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    fn unauthorized(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: msg.into(),
        }
    }
}

impl From<libation_library::LibraryError> for PortalError {
    fn from(value: libation_library::LibraryError) -> Self {
        Self::bad(value.to_string())
    }
}

impl From<crate::error::IntegrationError> for PortalError {
    fn from(value: crate::error::IntegrationError) -> Self {
        Self::bad(value.to_string())
    }
}

impl IntoResponse for PortalError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}
