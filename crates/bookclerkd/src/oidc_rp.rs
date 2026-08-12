//! OIDC/OAuth relying party (identity broker) for first-party User login.
//!
//! Bookclerk remains the authorization server for Audiobookshelf. These routes
//! consume upstream IdPs, JIT/link Users, and mint portal sessions. The Operator
//! account is never an OAuth subject.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use bookclerk_config::{
    allowlist_permits, email_domain_allowed, resolve_mapped_role, OidcProviderConfig,
    OidcProvisionMode,
};
use bookclerk_library::{hash_token, LibraryError, UserRecord, UserRole, UserStatus};
use chrono::{Duration as ChronoDuration, Utc};
use rand::RngCore;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::{
    issue_elevation, issue_portal_session, resolve_portal_caller_identity,
    timed_portal_identity_from_headers,
};

/// RP routes (`/api/auth/oidc/*`).
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/auth/oidc/providers", get(list_providers))
        .route("/api/auth/oidc/identities", get(list_identities))
        .route("/api/auth/oidc/login", get(login_start))
        .route("/api/auth/oidc/elevate", get(elevate_start))
        .route("/api/auth/oidc/callback", get(callback))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct ProviderQuery {
    provider: Option<String>,
}

async fn list_providers(State(state): State<Arc<AppState>>) -> Json<Value> {
    let cfg = state.config.read().await;
    let providers: Vec<Value> = cfg
        .auth
        .oidc
        .enabled_providers()
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.display_name(),
            })
        })
        .collect();
    Json(serde_json::json!({
        "enabled": cfg.auth.oidc.enabled,
        "providers": providers,
    }))
}

async fn list_identities(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let library = state.library_snapshot().await;
    let identity = timed_portal_identity_from_headers(&library, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let (_, _, user, _) = resolve_portal_caller_identity(&library, &identity).await;
    let user = user.ok_or(StatusCode::UNAUTHORIZED)?;
    let rows = library
        .list_portal_identities_for_user(user.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let identities: Vec<Value> = rows
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "provider": p.provider,
                "external_user_id": p.external_user_id,
                "label": p.label,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "identities": identities })))
}

async fn login_start(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ProviderQuery>,
) -> Result<Response, StatusCode> {
    start_authorize(&state, q.provider.as_deref(), "login", None).await
}

async fn elevate_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ProviderQuery>,
) -> Result<Response, StatusCode> {
    let library = state.library_snapshot().await;
    let identity = timed_portal_identity_from_headers(&library, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let (role, _, user, _) = resolve_portal_caller_identity(&library, &identity).await;
    if role != "owner" {
        return Err(StatusCode::FORBIDDEN);
    }
    let user = user.ok_or(StatusCode::FORBIDDEN)?;
    let provider_id = match q
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => id.to_string(),
        None => {
            let linked = library
                .list_portal_identities_for_user(user.id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            linked
                .into_iter()
                .find_map(|p| p.provider.strip_prefix("oidc:").map(str::to_string))
                .ok_or(StatusCode::BAD_REQUEST)?
        }
    };
    start_authorize(&state, Some(&provider_id), "elevate", Some(user.id)).await
}

async fn start_authorize(
    state: &AppState,
    provider_id: Option<&str>,
    purpose: &str,
    user_id: Option<i64>,
) -> Result<Response, StatusCode> {
    let cfg = state.config.read().await;
    if !cfg.auth.oidc.enabled {
        return Err(StatusCode::NOT_FOUND);
    }
    let id = provider_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let provider = cfg
        .auth
        .oidc
        .provider(id)
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    let origin = public_origin(&cfg);
    drop(cfg);
    let endpoints = resolve_endpoints(&provider)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let state_raw = Uuid::new_v4().to_string();
    let nonce = random_b64url(32);
    let verifier = random_b64url(32);
    let challenge = pkce_challenge(&verifier);
    let library = state.library_snapshot().await;
    library
        .insert_oidc_rp_state(
            &hash_token(&state_raw),
            provider.id.trim(),
            &verifier,
            &nonce,
            purpose,
            user_id,
            Utc::now() + ChronoDuration::minutes(10),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let redirect_uri = format!("{origin}/api/auth/oidc/callback");
    let mut url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&state={}&scope={}&code_challenge={}&code_challenge_method=S256",
        endpoints.authorize,
        urlencoding(&provider.client_id),
        urlencoding(&redirect_uri),
        urlencoding(&state_raw),
        urlencoding(&provider.effective_scopes().join(" ")),
        urlencoding(&challenge),
    );
    if provider.effective_scopes().iter().any(|s| s == "openid") {
        url.push_str("&nonce=");
        url.push_str(&urlencoding(&nonce));
    }
    if purpose == "elevate" {
        url.push_str("&prompt=login");
    }
    Ok(Redirect::temporary(&url).into_response())
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Result<Response, StatusCode> {
    if q.error.is_some() {
        return Ok(Redirect::temporary("/?sso_error=denied").into_response());
    }
    let code = q
        .code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let state_raw = q
        .state
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let library = state.library_snapshot().await;
    let Some((provider_id, verifier, nonce, purpose, elevate_user_id)) = library
        .take_oidc_rp_state(&hash_token(state_raw))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Ok(Redirect::temporary("/?sso_error=expired").into_response());
    };
    let cfg = state.config.read().await;
    let Some(provider) = cfg.auth.oidc.provider(&provider_id).cloned() else {
        return Err(StatusCode::NOT_FOUND);
    };
    let origin = public_origin(&cfg);
    let global_domains = cfg.auth.oidc.allowed_email_domains.clone();
    drop(cfg);
    let endpoints = resolve_endpoints(&provider)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let secret = client_secret(&state, &provider).await;
    let redirect_uri = format!("{origin}/api/auth/oidc/callback");
    let token_json = exchange_code(
        &endpoints.token,
        &provider.client_id,
        secret.as_deref(),
        code,
        &redirect_uri,
        &verifier,
    )
    .await
    .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let access_token = token_json
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or(StatusCode::BAD_GATEWAY)?;
    let id_token = token_json.get("id_token").and_then(Value::as_str);
    if let Some(id_token) = id_token {
        if let Some(claims) = decode_jwt_payload(id_token) {
            if let Some(got) = claims.get("nonce").and_then(Value::as_str) {
                if got != nonce {
                    return Ok(Redirect::temporary("/?sso_error=nonce").into_response());
                }
            }
        }
    }
    let userinfo = fetch_userinfo(&endpoints.userinfo, access_token)
        .await
        .unwrap_or(Value::Null);
    let mut profile =
        UpstreamProfile::from_tokens(id_token, &userinfo).ok_or(StatusCode::BAD_GATEWAY)?;
    if profile.email.is_none()
        && provider
            .preset
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("github"))
    {
        if let Ok(email) = fetch_github_email(access_token).await {
            profile.email = email;
        }
    }
    if purpose == "elevate" {
        let user_id = elevate_user_id.ok_or(StatusCode::BAD_REQUEST)?;
        let expected = format!("{}|{}", provider.portal_provider(), profile.sub);
        let linked = library
            .list_portal_identities_for_user(user_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let ok = linked.iter().any(|p| {
            p.provider == provider.portal_provider() && p.external_user_id == profile.sub
                || format!("{}|{}", p.provider, p.external_user_id) == expected
        });
        if !ok {
            return Ok(Redirect::temporary("/settings?sso_error=mismatch").into_response());
        }
        let user = library
            .get_user(user_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)?;
        if user.role != UserRole::Owner {
            return Err(StatusCode::FORBIDDEN);
        }
        let res = issue_elevation(&state, &library, user.id, &headers).await?;
        // Prefer sending the Owner back to Settings after step-up.
        if res.status() == StatusCode::OK {
            if let Some(cookie) = res.headers().get(header::SET_COOKIE).cloned() {
                return Ok((
                    StatusCode::SEE_OTHER,
                    [
                        (header::SET_COOKIE, cookie),
                        (
                            header::LOCATION,
                            axum::http::HeaderValue::from_static("/settings"),
                        ),
                    ],
                )
                    .into_response());
            }
        }
        return Ok(res);
    }

    match provision_user(
        &state,
        &library,
        &provider,
        &global_domains,
        &profile,
        id_token,
        &userinfo,
    )
    .await
    {
        Ok(user) => {
            if matches!(user.status, UserStatus::Disabled) {
                return Ok(Redirect::temporary("/?sso_error=disabled").into_response());
            }
            let issued =
                issue_portal_session(&state, &library, &user, &headers, "oidc_login").await?;
            if let Some(cookie) = issued.headers().get(header::SET_COOKIE).cloned() {
                return Ok((
                    StatusCode::SEE_OTHER,
                    [
                        (header::SET_COOKIE, cookie),
                        (header::LOCATION, axum::http::HeaderValue::from_static("/")),
                    ],
                )
                    .into_response());
            }
            Ok(issued)
        }
        Err(ProvisionError::Denied) => {
            Ok(Redirect::temporary("/?sso_error=no_role").into_response())
        }
        Err(ProvisionError::Conflict) => {
            Ok(Redirect::temporary("/?sso_error=conflict").into_response())
        }
        Err(ProvisionError::Internal) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

enum ProvisionError {
    Denied,
    Conflict,
    Internal,
}

async fn provision_user(
    state: &AppState,
    library: &bookclerk_library::LibraryStore,
    provider: &OidcProviderConfig,
    global_domains: &[String],
    profile: &UpstreamProfile,
    id_token: Option<&str>,
    userinfo: &Value,
) -> Result<UserRecord, ProvisionError> {
    let portal_provider = provider.portal_provider();
    if !email_domain_allowed(profile.email.as_deref(), global_domains) {
        return Err(ProvisionError::Denied);
    }
    if !matches!(provider.provision, OidcProvisionMode::Allowlist)
        && !email_domain_allowed(profile.email.as_deref(), &provider.allowed_email_domains)
    {
        return Err(ProvisionError::Denied);
    }
    let claim_values = collect_role_claims(id_token, userinfo, &provider.role_claim);
    let mapped = resolve_mapped_role(&provider.role_map, &claim_values);
    let desired_role = match provider.provision {
        OidcProvisionMode::MappedRole => {
            let raw = mapped.as_deref().ok_or(ProvisionError::Denied)?;
            UserRole::parse(raw).ok_or(ProvisionError::Denied)?
        }
        OidcProvisionMode::Any => mapped
            .as_deref()
            .and_then(UserRole::parse)
            .unwrap_or_else(|| UserRole::parse(&provider.default_role).unwrap_or(UserRole::Member)),
        OidcProvisionMode::Allowlist => {
            if !allowlist_permits(
                profile.email.as_deref(),
                &profile.sub,
                &provider.allowed_emails,
                &provider.allowed_email_domains,
                &provider.allowed_subjects,
                global_domains,
            ) {
                return Err(ProvisionError::Denied);
            }
            mapped
                .as_deref()
                .and_then(UserRole::parse)
                .unwrap_or_else(|| {
                    UserRole::parse(&provider.default_role).unwrap_or(UserRole::Member)
                })
        }
        OidcProvisionMode::InviteOnly => mapped
            .as_deref()
            .and_then(UserRole::parse)
            .unwrap_or(UserRole::Member),
    };

    if let Some(existing) = library
        .get_portal_identity(&portal_provider, &profile.sub)
        .await
        .map_err(|_| ProvisionError::Internal)?
    {
        let uid = existing.user_id.ok_or(ProvisionError::Internal)?;
        let mut user = library
            .get_user(uid)
            .await
            .map_err(|_| ProvisionError::Internal)?
            .ok_or(ProvisionError::Internal)?;
        if (matches!(provider.provision, OidcProvisionMode::MappedRole) || mapped.is_some())
            && user.role != desired_role
        {
            match library.set_user_role(user.id, desired_role).await {
                Ok(updated) => user = updated,
                Err(LibraryError::LastOwner) => {
                    let _ = library
                        .insert_security_audit_event(
                            &format!("user:{}", user.id),
                            "oidc_role_sync_blocked",
                            Some(r#"{"reason":"last_owner"}"#),
                        )
                        .await;
                }
                Err(_) => return Err(ProvisionError::Internal),
            }
        }
        refresh_profile(library, &user, profile).await?;
        return library
            .get_user(user.id)
            .await
            .map_err(|_| ProvisionError::Internal)?
            .ok_or(ProvisionError::Internal);
    }

    if provider.link_by_email {
        if let Some(email) = profile.email.as_deref() {
            if let Some(existing) = library
                .get_user_by_email(email)
                .await
                .map_err(|_| ProvisionError::Internal)?
            {
                if matches!(existing.status, UserStatus::Disabled) {
                    return Err(ProvisionError::Denied);
                }
                library
                    .link_portal_identity_to_user(
                        &portal_provider,
                        &profile.sub,
                        existing.id,
                        profile.name.as_deref(),
                    )
                    .await
                    .map_err(|_| ProvisionError::Conflict)?;
                let mut user = existing;
                if (matches!(provider.provision, OidcProvisionMode::MappedRole) || mapped.is_some())
                    && user.role != desired_role
                {
                    match library.set_user_role(user.id, desired_role).await {
                        Ok(updated) => user = updated,
                        Err(LibraryError::LastOwner) => {}
                        Err(_) => return Err(ProvisionError::Internal),
                    }
                }
                refresh_profile(library, &user, profile).await?;
                return library
                    .get_user(user.id)
                    .await
                    .map_err(|_| ProvisionError::Internal)?
                    .ok_or(ProvisionError::Internal);
            }
        }
    }

    if matches!(provider.provision, OidcProvisionMode::InviteOnly) {
        return Err(ProvisionError::Denied);
    }

    let user = library
        .create_user_with_profile(
            desired_role,
            profile.name.as_deref(),
            None,
            profile.email.as_deref(),
            None,
        )
        .await
        .map_err(|_| ProvisionError::Conflict)?;
    library
        .link_portal_identity_to_user(
            &portal_provider,
            &profile.sub,
            user.id,
            profile.name.as_deref(),
        )
        .await
        .map_err(|_| ProvisionError::Internal)?;
    let _ = library
        .insert_security_audit_event(
            &format!("user:{}", user.id),
            "oidc_jit",
            Some(&format!(
                r#"{{"provider":"{}","role":"{}"}}"#,
                provider.id,
                desired_role.as_str()
            )),
        )
        .await;
    let _ = state;
    Ok(user)
}

async fn refresh_profile(
    library: &bookclerk_library::LibraryStore,
    user: &UserRecord,
    profile: &UpstreamProfile,
) -> Result<(), ProvisionError> {
    if let Some(name) = profile
        .name
        .as_deref()
        .filter(|&n| user.display_name.as_deref() != Some(n))
    {
        let _ = library.set_user_display_name(user.id, Some(name)).await;
    }
    if let Some(email) = profile
        .email
        .as_deref()
        .filter(|&e| user.email.as_deref() != Some(e))
    {
        let _ = library.set_user_email(user.id, Some(email)).await;
    }
    Ok(())
}

struct UpstreamProfile {
    sub: String,
    email: Option<String>,
    name: Option<String>,
}

impl UpstreamProfile {
    fn from_tokens(id_token: Option<&str>, userinfo: &Value) -> Option<Self> {
        let id_claims = id_token.and_then(decode_jwt_payload);
        let sub = userinfo
            .get("sub")
            .and_then(Value::as_str)
            .or_else(|| userinfo.get("id").and_then(|v| v.as_i64().map(|_| "")))
            .map(str::to_string)
            .or_else(|| {
                userinfo.get("id").and_then(|v| match v {
                    Value::Number(n) => Some(n.to_string()),
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
            })
            .or_else(|| {
                id_claims
                    .as_ref()
                    .and_then(|c| c.get("sub").and_then(Value::as_str).map(str::to_string))
            })?;
        if sub.trim().is_empty() {
            return None;
        }
        let email = userinfo
            .get("email")
            .and_then(Value::as_str)
            .or_else(|| {
                id_claims
                    .as_ref()
                    .and_then(|c| c.get("email").and_then(Value::as_str))
            })
            .map(str::to_string);
        let name = userinfo
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| userinfo.get("login").and_then(Value::as_str))
            .or_else(|| {
                id_claims
                    .as_ref()
                    .and_then(|c| c.get("name").and_then(Value::as_str))
            })
            .map(str::to_string);
        Some(Self { sub, email, name })
    }
}

struct Endpoints {
    authorize: String,
    token: String,
    userinfo: String,
}

async fn resolve_endpoints(provider: &OidcProviderConfig) -> Result<Endpoints, ()> {
    if let Some(preset) = provider.preset.as_deref().map(str::trim) {
        match preset {
            "google" => {
                return discovery("https://accounts.google.com").await;
            }
            "github" => {
                return Ok(Endpoints {
                    authorize: "https://github.com/login/oauth/authorize".into(),
                    token: "https://github.com/login/oauth/access_token".into(),
                    userinfo: "https://api.github.com/user".into(),
                });
            }
            "apple" => {
                return discovery("https://appleid.apple.com").await;
            }
            "discord" => {
                return Ok(Endpoints {
                    authorize: "https://discord.com/api/oauth2/authorize".into(),
                    token: "https://discord.com/api/oauth2/token".into(),
                    userinfo: "https://discord.com/api/users/@me".into(),
                });
            }
            _ => {}
        }
    }
    let issuer = provider
        .issuer
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(())?;
    discovery(issuer).await
}

async fn discovery(issuer: &str) -> Result<Endpoints, ()> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let resp: Value = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?
        .json()
        .await
        .map_err(|_| ())?;
    Ok(Endpoints {
        authorize: resp
            .get("authorization_endpoint")
            .and_then(Value::as_str)
            .ok_or(())?
            .to_string(),
        token: resp
            .get("token_endpoint")
            .and_then(Value::as_str)
            .ok_or(())?
            .to_string(),
        userinfo: resp
            .get("userinfo_endpoint")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

async fn exchange_code(
    token_url: &str,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<Value, ()> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }
    let resp = reqwest::Client::new()
        .post(token_url)
        .header(header::ACCEPT, "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?;
    resp.json().await.map_err(|_| ())
}

async fn fetch_userinfo(url: &str, access_token: &str) -> Result<Value, ()> {
    if url.trim().is_empty() {
        return Err(());
    }
    reqwest::Client::new()
        .get(url)
        .bearer_auth(access_token)
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, "bookclerk")
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?
        .json()
        .await
        .map_err(|_| ())
}

async fn client_secret(state: &AppState, provider: &OidcProviderConfig) -> Option<String> {
    let env_key = format!(
        "BOOKCLERK_OIDC_{}_CLIENT_SECRET",
        provider.id.trim().to_ascii_uppercase().replace('-', "_")
    );
    if let Ok(v) = std::env::var(env_key) {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    if let Some(s) = provider
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(s.to_string());
    }
    let library = state.library_snapshot().await;
    let store = bookclerk_library::SecretStore::new(library.db());
    if let Ok(Some(row)) = store
        .get(
            bookclerk_library::secret_kind::OIDC_CLIENT,
            Some("oidc"),
            bookclerk_library::secret_account_type::OPERATOR,
            Some("operator"),
            provider.id.trim(),
        )
        .await
    {
        if let Ok(plain) = bookclerk_library::unseal_secret(&row) {
            let s = String::from_utf8_lossy(&plain).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

async fn fetch_github_email(access_token: &str) -> Result<Option<String>, ()> {
    let emails: Value = reqwest::Client::new()
        .get("https://api.github.com/user/emails")
        .bearer_auth(access_token)
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, "bookclerk")
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?
        .json()
        .await
        .map_err(|_| ())?;
    let Some(arr) = emails.as_array() else {
        return Ok(None);
    };
    let mut fallback = None;
    for row in arr {
        let Some(email) = row.get("email").and_then(Value::as_str) else {
            continue;
        };
        let verified = row
            .get("verified")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let primary = row.get("primary").and_then(Value::as_bool).unwrap_or(false);
        if verified && primary {
            return Ok(Some(email.to_string()));
        }
        if verified && fallback.is_none() {
            fallback = Some(email.to_string());
        }
    }
    Ok(fallback)
}

fn public_origin(cfg: &bookclerk_config::Config) -> String {
    cfg.integrations
        .public_origin
        .as_deref()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| String::from("http://127.0.0.1:8787"))
}

fn collect_role_claims(id_token: Option<&str>, userinfo: &Value, claim: &str) -> Vec<String> {
    let mut out = Vec::new();
    push_claim_values(userinfo, claim, &mut out);
    if let Some(claims) = id_token.and_then(decode_jwt_payload) {
        push_claim_values(&claims, claim, &mut out);
        if let Some(roles) = claims
            .pointer("/realm_access/roles")
            .and_then(Value::as_array)
        {
            for v in roles {
                if let Some(s) = v.as_str() {
                    out.push(s.to_string());
                }
            }
        }
    }
    out
}

fn push_claim_values(obj: &Value, claim: &str, out: &mut Vec<String>) {
    match obj.get(claim) {
        Some(Value::String(s)) => out.push(s.clone()),
        Some(Value::Array(arr)) => {
            for v in arr {
                if let Some(s) = v.as_str() {
                    out.push(s.to_string());
                }
            }
        }
        _ => {}
    }
}

fn decode_jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn random_b64url(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn pkce_is_s256() {
        let v = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(v),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn mapped_role_from_userinfo_groups() {
        let mut map = BTreeMap::new();
        map.insert("bookclerk-admins".into(), "administrator".into());
        map.insert("bookclerk-users".into(), "member".into());
        let info = serde_json::json!({"groups": ["bookclerk-users", "bookclerk-admins"]});
        let claims = collect_role_claims(None, &info, "groups");
        assert_eq!(
            resolve_mapped_role(&map, &claims).as_deref(),
            Some("administrator")
        );
    }

    #[test]
    fn urlencoding_percent_encodes_space() {
        assert_eq!(urlencoding("openid profile"), "openid%20profile");
    }
}

#[cfg(test)]
mod http_tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use bookclerk_config::{OidcProviderConfig, OidcProvisionMode};
    use tower::ServiceExt;

    use super::*;
    use crate::auth::OperatorAuthState;

    async fn harness(
        oidc_enabled: bool,
        providers: Vec<OidcProviderConfig>,
    ) -> (Arc<AppState>, axum::Router, bookclerk_library::LibraryStore) {
        use bookclerk_config::{Config, ListenAddrs};
        use bookclerk_integrations::IntegrationRegistry;
        use bookclerk_library::LibraryStore;
        use bookclerk_plugin_host::{DatabaseRegistry, DestinationRegistry};
        use bookclerk_source::SourceRegistry;
        use tokio::sync::{Mutex, Notify, RwLock, Semaphore};

        let library = LibraryStore::from_connection(
            bookclerk_plugin_database_sqlite::open_memory()
                .await
                .expect("sqlite memory"),
        );
        let _ = library.ensure_users_bridged().await;
        let mut cfg = Config::default();
        cfg.daemon.listen = ListenAddrs::parse_list("127.0.0.1:8787").unwrap();
        cfg.daemon.auth.enabled = true;
        cfg.auth.oidc.enabled = oidc_enabled;
        cfg.auth.oidc.providers = providers;

        let state = Arc::new(AppState {
            config: Arc::new(RwLock::new(cfg)),
            library: Arc::new(RwLock::new(library.clone())),
            database_registry: Arc::new(RwLock::new(DatabaseRegistry::default())),
            jobs: Arc::new(RwLock::new(Vec::new())),
            work_lock: Mutex::new(()),
            discover_gate: Arc::new(Semaphore::new(1)),
            integrations: Arc::new(RwLock::new(IntegrationRegistry::new())),
            sources: Arc::new(RwLock::new(SourceRegistry::new())),
            destinations: Arc::new(RwLock::new(DestinationRegistry::default())),
            auth: Arc::new(RwLock::new(Arc::new(OperatorAuthState::new(
                "op-token-oidc".into(),
                12,
                true,
                5,
                30,
            )))),
            reload_lock: Mutex::new(()),
            listen_reload: Arc::new(Notify::new()),
            last_bound_listen: RwLock::new(None),
            tray: RwLock::new(None),
        });
        let app = crate::api::router(state.clone(), None);
        (state, app, library)
    }

    fn github_provider() -> OidcProviderConfig {
        OidcProviderConfig {
            id: "github".into(),
            name: "GitHub".into(),
            preset: Some("github".into()),
            client_id: "test-client".into(),
            provision: OidcProvisionMode::Any,
            ..OidcProviderConfig::default()
        }
    }

    #[tokio::test]
    async fn providers_hidden_when_disabled() {
        let (_state, app, _library) = harness(false, vec![github_provider()]).await;
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/oidc/providers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = http_body_util::BodyExt::collect(res.into_body())
            .await
            .unwrap()
            .to_bytes();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["enabled"], false);
        assert_eq!(json["providers"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn github_login_redirects_to_authorize() {
        let (_state, app, _library) = harness(true, vec![github_provider()]).await;
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/oidc/login?provider=github")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
        let loc = res
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            loc.starts_with("https://github.com/login/oauth/authorize?"),
            "{loc}"
        );
        assert!(loc.contains("client_id=test-client"), "{loc}");
        assert!(loc.contains("code_challenge_method=S256"), "{loc}");
        assert!(!loc.contains("prompt=login"), "{loc}");
    }

    #[tokio::test]
    async fn elevate_requires_owner_session() {
        let (_state, app, _library) = harness(true, vec![github_provider()]).await;
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/oidc/elevate?provider=github")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn elevate_owner_redirects_with_prompt_login() {
        use bookclerk_library::{hash_token, UserRole};
        use chrono::{Duration as ChronoDuration, Utc};
        use uuid::Uuid;

        let (_state, app, library) = harness(true, vec![github_provider()]).await;
        let owner = library
            .create_user(UserRole::Owner, Some("Owner"), None)
            .await
            .unwrap();
        library
            .link_portal_identity_to_user("oidc:github", "gh-sub", owner.id, Some("Owner"))
            .await
            .unwrap();
        let identity = library
            .ensure_local_portal_identity(owner.id, Some("Owner"))
            .await
            .unwrap();
        let raw = Uuid::new_v4().to_string();
        library
            .insert_portal_session(
                &hash_token(&raw),
                identity.id,
                Utc::now() + ChronoDuration::hours(12),
            )
            .await
            .unwrap();
        let cookie = format!("{}={raw}", crate::auth::PORTAL_SESSION_COOKIE);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/oidc/elevate?provider=github")
                    .header(axum::http::header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
        let loc = res
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(loc.contains("prompt=login"), "{loc}");
        assert!(
            loc.starts_with("https://github.com/login/oauth/authorize?"),
            "{loc}"
        );
    }

    #[tokio::test]
    async fn login_unknown_provider_is_not_found() {
        let (_state, app, _library) = harness(true, vec![github_provider()]).await;
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/oidc/login?provider=nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
