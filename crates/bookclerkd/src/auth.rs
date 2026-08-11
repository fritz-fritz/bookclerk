//! Operator and portal session authentication for the daemon HTTP API.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::connect_info::ConnectInfo;
use axum::extract::Query;
use axum::extract::Request;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use bookclerk_config::session_cookie_flags;
use bookclerk_integrations::portal_identity_from_headers;
use bookclerk_library::{
    hash_token, portal_prefs_key, user_prefs_key, PortalIdentity, UserRole, OPERATOR_PREFS_KEY,
};
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::time::timeout;
use uuid::Uuid;

use crate::api::AppState;

pub const SESSION_COOKIE: &str = "bookclerk_operator_session";
pub const PORTAL_SESSION_COOKIE: &str = "bookclerk_portal_session";
const AUTH_DB_TIMEOUT: Duration = Duration::from_secs(3);

/// Peer IP for login throttling (`ConnectInfo` when available, else `"unknown"`).
pub(crate) struct ClientIp(String);

impl FromRequestParts<Arc<AppState>> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        let trusted = state.config.read().await.daemon.trusted_proxies.clone();
        Ok(Self(resolve_client_ip_key(peer, &parts.headers, &trusted)))
    }
}

/// Resolve the throttle/key client identity, honoring trusted reverse proxies.
fn resolve_client_ip_key(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trusted_proxies: &[String],
) -> String {
    let Some(peer) = peer else {
        return String::from("unknown");
    };
    if !peer_is_trusted(peer, trusted_proxies) {
        return peer.to_string();
    }
    if let Some(client) = forwarded_client_ip(headers) {
        return client;
    }
    peer.to_string()
}

fn peer_is_trusted(peer: IpAddr, trusted_proxies: &[String]) -> bool {
    trusted_proxies.iter().any(|entry| {
        let entry = entry.trim();
        if entry.is_empty() {
            return false;
        }
        if let Ok(cidr_ip) = entry.parse::<IpAddr>() {
            return cidr_ip == peer;
        }
        // Prefix match for simple CIDR forms like `10.0.0.0/8` (IPv4 only).
        if let Some((base, bits)) = entry.split_once('/') {
            if let (Ok(base_ip), Ok(prefix)) = (base.parse::<IpAddr>(), bits.parse::<u8>()) {
                return ip_in_prefix(peer, base_ip, prefix);
            }
        }
        false
    })
}

fn ip_in_prefix(ip: IpAddr, base: IpAddr, prefix: u8) -> bool {
    match (ip, base) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            if prefix > 32 {
                return false;
            }
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            (u32::from(a) & mask) == (u32::from(b) & mask)
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            if prefix > 128 {
                return false;
            }
            let a = u128::from(a);
            let b = u128::from(b);
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            (a & mask) == (b & mask)
        }
        _ => false,
    }
}

fn forwarded_client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        let first = xff.split(',').next()?.trim();
        if !first.is_empty() && first.parse::<IpAddr>().is_ok() {
            return Some(first.to_string());
        }
    }
    if let Some(fwd) = headers.get(header::FORWARDED).and_then(|v| v.to_str().ok()) {
        // Take the first `for=` value.
        for part in fwd.split(';') {
            let part = part.trim();
            if let Some(rest) = part
                .strip_prefix("for=")
                .or_else(|| part.strip_prefix("For="))
            {
                let candidate = rest.trim().trim_matches('"').trim_start_matches('[');
                let candidate = candidate.trim_end_matches(']');
                let host = candidate.split(':').next().unwrap_or(candidate);
                if host.parse::<IpAddr>().is_ok() {
                    return Some(host.to_string());
                }
            }
        }
    }
    None
}

#[derive(Debug)]
struct LoginThrottleBucket {
    failures: u32,
    window_start: Instant,
    locked_until: Option<Instant>,
}

/// How long a previous operator token remains valid after rotate/reload.
pub const OPERATOR_TOKEN_GRACE: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct OperatorAuthState {
    pub token: String,
    /// Prior token accepted until this deadline (rotate/reload overlap).
    previous_token: Option<(String, Instant)>,
    pub session_ttl: Duration,
    pub enabled: bool,
    login_max_failures: u32,
    login_window: Duration,
    login_lockout: Duration,
    login_attempts: Mutex<HashMap<String, LoginThrottleBucket>>,
}

impl OperatorAuthState {
    pub fn new(
        token: String,
        session_ttl_hours: u64,
        enabled: bool,
        login_max_failures: u32,
        login_lockout_secs: u64,
    ) -> Self {
        let login_lockout = Duration::from_secs(login_lockout_secs.max(1));
        Self {
            token,
            previous_token: None,
            session_ttl: Duration::from_secs(session_ttl_hours.saturating_mul(3600).max(3600)),
            enabled,
            login_max_failures: login_max_failures.max(1),
            // Failures count within the same period as the lockout by default.
            login_window: login_lockout,
            login_lockout,
            login_attempts: Mutex::new(HashMap::new()),
        }
    }

    /// Accept `previous`'s token (and any still-valid grace token) for [`OPERATOR_TOKEN_GRACE`].
    pub fn install_token_grace_from(&mut self, previous: &Self, grace: Duration) {
        if !self.enabled || !previous.enabled {
            return;
        }
        let now = Instant::now();
        if !previous.token.is_empty() && previous.token != self.token {
            self.previous_token = Some((previous.token.clone(), now + grace));
            return;
        }
        if let Some((tok, until)) = &previous.previous_token {
            if now < *until && tok != &self.token {
                self.previous_token = Some((tok.clone(), *until));
            }
        }
    }

    /// Preserve in-memory login throttle buckets across auth reload swaps.
    ///
    /// Operator sessions live in SQLite (`operator_sessions`) and do not need
    /// to be copied here.
    pub async fn take_session_state_from(&mut self, previous: &Self) {
        let mut old = previous.login_attempts.lock().await;
        let mut new = self.login_attempts.lock().await;
        *new = std::mem::take(&mut *old);
    }

    fn token_matches(&self, candidate: &str) -> bool {
        if constant_time_eq(self.token.as_bytes(), candidate.as_bytes()) {
            return true;
        }
        if let Some((prev, until)) = &self.previous_token {
            Instant::now() < *until && constant_time_eq(prev.as_bytes(), candidate.as_bytes())
        } else {
            false
        }
    }

    /// `None` = allowed; `Some(retry_after)` = locked out.
    async fn login_throttle_check(&self, client_key: &str) -> Option<Duration> {
        let mut map = self.login_attempts.lock().await;
        prune_login_attempts(&mut map, self.login_window, self.login_lockout);
        let bucket = map.get_mut(client_key)?;
        if let Some(until) = bucket.locked_until {
            let now = Instant::now();
            if now < until {
                return Some((until - now).max(Duration::from_secs(1)));
            }
            // Lockout expired — reset the window.
            bucket.failures = 0;
            bucket.locked_until = None;
            bucket.window_start = now;
        }
        None
    }

    async fn record_login_failure(&self, client_key: &str) -> Option<Duration> {
        let mut map = self.login_attempts.lock().await;
        prune_login_attempts(&mut map, self.login_window, self.login_lockout);
        let now = Instant::now();
        let bucket = map
            .entry(client_key.to_string())
            .or_insert_with(|| LoginThrottleBucket {
                failures: 0,
                window_start: now,
                locked_until: None,
            });
        if bucket.window_start.elapsed() >= self.login_window {
            bucket.failures = 0;
            bucket.window_start = now;
            bucket.locked_until = None;
        }
        bucket.failures = bucket.failures.saturating_add(1);
        if bucket.failures >= self.login_max_failures {
            bucket.locked_until = Some(now + self.login_lockout);
            return Some(self.login_lockout);
        }
        None
    }

    async fn clear_login_failures(&self, client_key: &str) {
        self.login_attempts.lock().await.remove(client_key);
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct TrayHandoffQuery {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct AuthMeResponse {
    pub authenticated: bool,
    /// `operator`, `administrator`, or `member` (legacy clients may still send `portal`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Configured post-auth landing view (`discover` / `library` / `accounts`).
    /// Loaded from per-user DB preferences (not config.toml).
    pub default_view: String,
    /// Whether this session may acquire / scan / manage jobs.
    /// True for operator and administrator.
    pub can_acquire: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal: Option<PortalMeInfo>,
    /// First-party user when the session is linked to a `users` row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<AuthMeUser>,
}

#[derive(Debug, Serialize)]
pub struct AuthMeUser {
    pub id: i64,
    pub role: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PortalMeInfo {
    pub identity_id: i64,
    pub provider: String,
    pub external_user_id: String,
    pub label: Option<String>,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    ok: bool,
    role: String,
    default_view: String,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    ClientIp(client_key): ClientIp,
    Json(body): Json<LoginRequest>,
) -> Result<Response, StatusCode> {
    let auth = state.auth_snapshot().await;
    let library = state.library_snapshot().await;
    let default_view = default_view_for_subject(&library, OPERATOR_PREFS_KEY, None).await;
    if !auth.enabled {
        return Ok((
            StatusCode::OK,
            [(
                header::SET_COOKIE,
                format!("{SESSION_COOKIE}=disabled; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"),
            )],
            Json(LoginResponse {
                ok: true,
                role: String::from("operator"),
                default_view,
            }),
        )
            .into_response());
    }
    // Locked-out clients are refused before the token is even compared.
    if let Some(retry_after) = auth.login_throttle_check(&client_key).await {
        return Ok(too_many_requests(retry_after));
    }

    if !auth.token_matches(body.token.trim()) {
        if let Some(retry_after) = auth.record_login_failure(&client_key).await {
            return Ok(too_many_requests(retry_after));
        }
        // Explicit JSON so the brand-error middleware keeps a precise message.
        return Ok((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "unauthorized",
                "message": "Invalid operator token.",
                "status": 401
            })),
        )
            .into_response());
    }

    auth.clear_login_failures(&client_key).await;
    Ok(issue_operator_session(&state, &auth, default_view).await)
}

/// Browser handoff from the system tray.
///
/// Linux `xdg-open` often strips URL fragments, so the tray opens this loopback
/// GET with the operator token as a query param. On success we set the session
/// cookie and redirect to `/` — the SPA never needs to parse a fragment.
pub async fn tray_handoff(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ClientIp(client_key): ClientIp,
    Query(query): Query<TrayHandoffQuery>,
) -> Result<Response, StatusCode> {
    if !addr.ip().is_loopback() {
        tracing::warn!(%addr, "tray handoff refused from non-loopback peer");
        return Err(StatusCode::FORBIDDEN);
    }

    let auth = state.auth_snapshot().await;

    if !auth.enabled {
        let mut res = Redirect::temporary("/").into_response();
        res.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_static(
                "bookclerk_operator_session=disabled; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
            ),
        );
        return Ok(res);
    }

    if let Some(retry_after) = auth.login_throttle_check(&client_key).await {
        return Ok(too_many_requests(retry_after));
    }

    if !auth.token_matches(query.token.trim()) {
        if let Some(retry_after) = auth.record_login_failure(&client_key).await {
            return Ok(too_many_requests(retry_after));
        }
        return Err(StatusCode::UNAUTHORIZED);
    }

    auth.clear_login_failures(&client_key).await;
    let cookie = persist_operator_session_cookie(&state, &auth).await;
    let mut res = Redirect::temporary("/").into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        res.headers_mut().insert(header::SET_COOKIE, value);
    }
    tracing::info!("tray handoff accepted; session cookie set");
    Ok(res)
}

async fn issue_operator_session(
    state: &AppState,
    auth: &OperatorAuthState,
    default_view: String,
) -> Response {
    let cookie = persist_operator_session_cookie(state, auth).await;
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(LoginResponse {
            ok: true,
            role: String::from("operator"),
            default_view,
        }),
    )
        .into_response()
}

async fn persist_operator_session_cookie(state: &AppState, auth: &OperatorAuthState) -> String {
    let session_id = Uuid::new_v4().to_string();
    let token_hash = hash_token(&session_id);
    let expires = Utc::now()
        + ChronoDuration::from_std(auth.session_ttl).unwrap_or_else(|_| ChronoDuration::hours(12));
    let library = state.library_snapshot().await;
    if let Err(err) = library.insert_operator_session(&token_hash, expires).await {
        tracing::error!(error = %err, "failed to persist operator session");
    }
    let _ = library.prune_expired_operator_sessions().await;
    let flags = {
        let cfg = state.config.read().await;
        session_cookie_flags(cfg.integrations.public_origin.as_deref())
    };
    let max_age = auth.session_ttl.as_secs();
    format!("{SESSION_COOKIE}={session_id}; {flags}; Max-Age={max_age}")
}

fn too_many_requests(retry_after: Duration) -> Response {
    let secs = retry_after.as_secs().max(1);
    let mut res = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "error": "too_many_requests",
            "message": format!(
                "Too many failed login attempts. Try again in {secs} second{}.",
                if secs == 1 { "" } else { "s" }
            ),
            "status": 429,
            "retry_after_secs": secs
        })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&secs.to_string()) {
        res.headers_mut().insert(header::RETRY_AFTER, value);
    }
    res
}

pub async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let library = state.library_snapshot().await;
    if let Some(session_id) = session_id_from_headers(&headers) {
        let hash = hash_token(&session_id);
        if let Err(err) = library.delete_operator_session(&hash).await {
            tracing::warn!(error = %err, "failed to revoke operator session");
        }
    }
    if let Some(portal_raw) = cookie_value(&headers, PORTAL_SESSION_COOKIE) {
        let hash = hash_token(&portal_raw);
        if let Err(err) = library.delete_portal_session(&hash).await {
            tracing::warn!(error = %err, "failed to revoke portal session on operator logout");
        }
    }
    let flags = {
        let cfg = state.config.read().await;
        session_cookie_flags(cfg.integrations.public_origin.as_deref())
    };
    let mut hdrs = HeaderMap::new();
    for cookie in [
        format!("{SESSION_COOKIE}=; {flags}; Max-Age=0"),
        format!("{PORTAL_SESSION_COOKIE}=; {flags}; Max-Age=0"),
    ] {
        if let Ok(v) = header::HeaderValue::from_str(&cookie) {
            hdrs.append(header::SET_COOKIE, v);
        }
    }
    (
        StatusCode::OK,
        hdrs,
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

pub async fn me(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    let auth = state.auth_snapshot().await;

    if !auth.enabled {
        let library = state.library_snapshot().await;
        let default_view = default_view_for_subject(&library, OPERATOR_PREFS_KEY, None).await;
        return (
            StatusCode::OK,
            Json(AuthMeResponse {
                authenticated: true,
                role: Some(String::from("operator")),
                default_view,
                can_acquire: true,
                portal: None,
                user: None,
            }),
        );
    }

    if authorize_operator(&state, &auth, &headers).await {
        let library = state.library_snapshot().await;
        let default_view = default_view_for_subject(&library, OPERATOR_PREFS_KEY, None).await;
        return (
            StatusCode::OK,
            Json(AuthMeResponse {
                authenticated: true,
                role: Some(String::from("operator")),
                default_view,
                can_acquire: true,
                portal: None,
                user: None,
            }),
        );
    }

    let library = state.library_snapshot().await;
    if let Some(identity) = timed_portal_identity_from_headers(&library, &headers).await {
        let (role, can_acquire, user, prefs_key) =
            resolve_portal_caller_identity(&library, &identity).await;
        let default_view =
            default_view_for_subject(&library, &prefs_key, Some(identity.id)).await;
        return (
            StatusCode::OK,
            Json(AuthMeResponse {
                authenticated: true,
                role: Some(role),
                default_view,
                can_acquire,
                portal: Some(PortalMeInfo {
                    identity_id: identity.id,
                    provider: identity.provider,
                    external_user_id: identity.external_user_id,
                    label: identity.label,
                }),
                user,
            }),
        );
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(AuthMeResponse {
            authenticated: false,
            role: None,
            default_view: String::from("discover"),
            can_acquire: false,
            portal: None,
            user: None,
        }),
    )
}

pub async fn require_operator_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Clone the auth Arc then drop the RwLock before `next.run` so a config
    // reload writer is not blocked for the full handler duration.
    let auth = state.auth_snapshot().await;
    let allowed = !auth.enabled || authorize_operator(&state, &auth, req.headers()).await;
    if allowed {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Allow operator sessions **or** portal sessions (Discover / read-only library).
pub async fn require_operator_or_portal_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth = state.auth_snapshot().await;
    let (allowed, check_portal) =
        if !auth.enabled || authorize_operator(&state, &auth, req.headers()).await {
            (true, false)
        } else {
            (false, true)
        };
    if allowed {
        return Ok(next.run(req).await);
    }
    if check_portal {
        let library = state.library_snapshot().await;
        if timed_portal_identity_from_headers(&library, req.headers())
            .await
            .is_some()
        {
            return Ok(next.run(req).await);
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

/// Resolve portal identity when the caller is not an operator.
pub async fn caller_portal_identity(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<PortalIdentity> {
    let auth = state.auth_snapshot().await;
    if !auth.enabled {
        return None;
    }
    if authorize_operator(state, &auth, headers).await {
        return None;
    }
    let library = state.library_snapshot().await;
    timed_portal_identity_from_headers(&library, headers).await
}

/// Subject key + optional portal identity id for the caller's preferences row.
///
/// Portal callers must resolve to their own prefs key. If the portal-identity
/// lookup times out or fails after portal auth, this returns an error instead of
/// falling back to [`OPERATOR_PREFS_KEY`].
pub async fn prefs_subject_for_caller(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(String, Option<i64>), StatusCode> {
    let auth = state.auth_snapshot().await;
    if !auth.enabled || authorize_operator(state, &auth, headers).await {
        return Ok((OPERATOR_PREFS_KEY.to_string(), None));
    }
    let library = state.library_snapshot().await;
    match timeout(
        AUTH_DB_TIMEOUT,
        portal_identity_from_headers(&library, headers),
    )
    .await
    {
        Ok(Some(identity)) => {
            let key = match identity.user_id {
                Some(user_id) => user_prefs_key(user_id),
                None => portal_prefs_key(identity.id),
            };
            Ok((key, Some(identity.id)))
        }
        Ok(None) => {
            tracing::warn!("portal identity missing for prefs subject");
            Err(StatusCode::UNAUTHORIZED)
        }
        Err(_) => {
            tracing::warn!("portal identity lookup timed out for prefs subject");
            Err(StatusCode::GATEWAY_TIMEOUT)
        }
    }
}

/// Map a portal session to first-party role / prefs subject / optional user info.
async fn resolve_portal_caller_identity(
    library: &bookclerk_library::LibraryStore,
    identity: &PortalIdentity,
) -> (String, bool, Option<AuthMeUser>, String) {
    let Some(user_id) = identity.user_id else {
        // Legacy unbridged portal identity — treat as member.
        return (
            String::from("member"),
            false,
            None,
            portal_prefs_key(identity.id),
        );
    };
    let prefs_key = user_prefs_key(user_id);
    match timeout(AUTH_DB_TIMEOUT, library.get_user(user_id)).await {
        Ok(Ok(Some(user))) => {
            let role = user.role.as_str().to_string();
            let can_acquire = matches!(user.role, UserRole::Administrator);
            let me_user = AuthMeUser {
                id: user.id,
                role: role.clone(),
                display_name: user.display_name.clone(),
            };
            (role, can_acquire, Some(me_user), prefs_key)
        }
        Ok(Ok(None)) | Ok(Err(_)) | Err(_) => {
            tracing::warn!(user_id, "linked user missing for portal identity");
            (String::from("member"), false, None, prefs_key)
        }
    }
}

async fn default_view_for_subject(
    library: &bookclerk_library::LibraryStore,
    subject_key: &str,
    identity_id: Option<i64>,
) -> String {
    match timeout(
        AUTH_DB_TIMEOUT,
        library.get_user_preferences_or_default(subject_key, identity_id),
    )
    .await
    {
        Ok(Ok(prefs)) => normalize_default_view(&prefs.default_view),
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "default view lookup failed");
            String::from("discover")
        }
        Err(_) => {
            tracing::warn!("default view lookup timed out");
            String::from("discover")
        }
    }
}

async fn timed_portal_identity_from_headers(
    library: &bookclerk_library::LibraryStore,
    headers: &HeaderMap,
) -> Option<PortalIdentity> {
    match timeout(
        AUTH_DB_TIMEOUT,
        portal_identity_from_headers(library, headers),
    )
    .await
    {
        Ok(identity) => identity,
        Err(_) => {
            tracing::warn!("portal identity lookup timed out");
            None
        }
    }
}

async fn authorize_operator(
    state: &AppState,
    auth: &OperatorAuthState,
    headers: &HeaderMap,
) -> bool {
    if let Some(token) = bearer_token(headers) {
        if auth.token_matches(token) {
            return true;
        }
    }
    let Some(session_id) = session_id_from_headers(headers) else {
        return false;
    };
    let token_hash = hash_token(&session_id);
    let library = state.library_snapshot().await;
    match timeout(AUTH_DB_TIMEOUT, library.operator_session_valid(&token_hash)).await {
        Ok(Ok(valid)) => valid,
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "operator session lookup failed");
            false
        }
        Err(_) => {
            tracing::warn!("operator session lookup timed out");
            false
        }
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    let trimmed = token.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    cookie_value(headers, SESSION_COOKIE)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(&format!("{name}=")) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn prune_login_attempts(
    attempts: &mut HashMap<String, LoginThrottleBucket>,
    window: Duration,
    lockout: Duration,
) {
    let now = Instant::now();
    attempts.retain(|_, bucket| {
        if let Some(until) = bucket.locked_until {
            if now < until {
                return true;
            }
        }
        bucket.window_start.elapsed() < window.max(lockout)
    });
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[must_use]
pub fn normalize_default_view(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "library" => String::from("library"),
        "accounts" => String::from("accounts"),
        "wishlist" => String::from("wishlist"),
        _ => String::from("discover"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn login_throttle_locks_after_max_failures() {
        let auth = OperatorAuthState::new("secret-token".into(), 12, true, 3, 30);
        assert!(auth.login_throttle_check("127.0.0.1").await.is_none());
        assert!(auth.record_login_failure("127.0.0.1").await.is_none());
        assert!(auth.record_login_failure("127.0.0.1").await.is_none());
        let locked = auth.record_login_failure("127.0.0.1").await;
        assert!(locked.is_some());
        // A locked bucket refuses further attempts even with a valid token.
        let retry = auth.login_throttle_check("127.0.0.1").await;
        assert!(retry.is_some());
        // Throttling is per client key.
        assert!(auth.login_throttle_check("10.0.0.2").await.is_none());
        auth.clear_login_failures("127.0.0.1").await;
        assert!(auth.login_throttle_check("127.0.0.1").await.is_none());
    }

    #[test]
    fn too_many_requests_sets_retry_after_and_json() {
        let res = too_many_requests(Duration::from_secs(45));
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(res.headers().get(header::RETRY_AFTER).unwrap(), "45");
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn token_grace_accepts_previous_until_deadline() {
        let previous = OperatorAuthState::new("old-token-value".into(), 12, true, 5, 30);
        let mut next = OperatorAuthState::new("new-token-value".into(), 12, true, 5, 30);
        next.install_token_grace_from(&previous, Duration::from_secs(30));
        assert!(next.token_matches("new-token-value"));
        assert!(next.token_matches("old-token-value"));
        assert!(!next.token_matches("other-token-value"));
    }

    #[test]
    fn trusted_proxy_uses_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.10, 10.0.0.1"),
        );
        let peer = "10.0.0.1".parse().unwrap();
        let key = resolve_client_ip_key(Some(peer), &headers, &["10.0.0.1".into()]);
        assert_eq!(key, "203.0.113.10");
        let untrusted = resolve_client_ip_key(Some(peer), &headers, &[]);
        assert_eq!(untrusted, "10.0.0.1");
    }
}
