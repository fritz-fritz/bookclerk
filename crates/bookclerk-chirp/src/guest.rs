//! Guest-process helpers for the external Chirp source plugin.
//!
//! These APIs do **not** open the library DB — the host seals credentials and
//! upserts scan DTOs via [`bookclerk_plugin::ExternalSource`].

use std::collections::BTreeMap;
use std::path::Path;

use bookclerk_library::NewBook;
use bookclerk_source::{LoginOptions, PlainFetch};
use serde_json::Value;

use crate::auth::ChirpAuthFile;
use crate::client::{ChirpClient, DEFAULT_GRAPHQL_URL};
use crate::download::fetch_title_materials;
use crate::error::{ChirpError, Result};
use crate::source::ID;
use crate::sync::collect_account_books;

/// Default GraphQL page size used when the host does not specify one.
const DEFAULT_GUEST_PAGE_SIZE: u32 = 20;

/// Login against Chirp and return account metadata + credential JSON.
///
/// Does not write to `encrypted_secrets` — the host seals `credentials`.
pub async fn guest_login(
    graphql_url: &str,
    opts: LoginOptions,
) -> Result<(String, String, Option<String>, bool, Value)> {
    let email = opts
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChirpError::auth("Chirp login requires email"))?;
    let password = opts
        .password
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChirpError::auth("Chirp login requires password"))?;

    let mut client = ChirpClient::new(graphql_url);
    let user = client.login(email, password).await?;

    let marketplace = if opts.marketplace.trim().is_empty() {
        String::from("us")
    } else {
        opts.marketplace.trim().to_ascii_lowercase()
    };

    let auth = ChirpAuthFile {
        access_token: user.token,
        web_token: user.web_token,
        email: user.email,
        user_id: Some(user.id),
        marketplace: marketplace.clone(),
        label: opts.label.clone(),
    };
    let account_id = auth.account_id().to_string();
    let label = auth.label.clone().or_else(|| Some(auth.email.clone()));
    let credentials = serde_json::to_value(&auth)
        .map_err(|e| ChirpError::auth(format!("serialize Chirp auth: {e}")))?;
    Ok((account_id, marketplace, label, true, credentials))
}

/// Scan libraries for the credential blobs the host injected.
pub async fn guest_scan(
    graphql_url: &str,
    credentials: &BTreeMap<String, Value>,
    account_filter: &[String],
    page_size: u32,
) -> Result<(Vec<NewBook>, usize, u32)> {
    if credentials.is_empty() {
        return Err(ChirpError::no_accounts(
            "no Chirp credentials from host — run login first",
        ));
    }
    let explicit = !account_filter.is_empty();
    let page_size = if page_size == 0 {
        DEFAULT_GUEST_PAGE_SIZE
    } else {
        page_size
    };
    let mut books = Vec::new();
    let mut accounts = 0usize;
    let mut pages = 0u32;

    for (account_id, creds) in credentials {
        if explicit
            && !account_filter
                .iter()
                .any(|n| n.eq_ignore_ascii_case(account_id))
        {
            continue;
        }
        let auth: ChirpAuthFile = serde_json::from_value(creds.clone()).map_err(|e| {
            ChirpError::auth(format!("invalid Chirp credentials for {account_id}: {e}"))
        })?;
        let client = ChirpClient::new(graphql_url).with_token(&auth.access_token);
        let (batch, p) =
            collect_account_books(&client, account_id, &auth.marketplace, page_size).await?;
        pages = pages.saturating_add(p);
        accounts += 1;
        books.extend(batch);
    }

    if accounts == 0 {
        return Err(ChirpError::no_accounts(
            "no matching Chirp accounts in host credentials",
        ));
    }
    Ok((books, accounts, pages))
}

/// Download one title into `cache_dir` using host-injected credentials.
pub async fn guest_fetch_title(
    graphql_url: &str,
    credentials: &Value,
    title_id: &str,
    cache_dir: &Path,
) -> Result<PlainFetch> {
    let auth: ChirpAuthFile = serde_json::from_value(credentials.clone())
        .map_err(|e| ChirpError::auth(format!("invalid Chirp credentials: {e}")))?;
    let client = ChirpClient::new(graphql_url).with_token(&auth.access_token);
    fetch_title_materials(&client, title_id, cache_dir).await
}

/// Resolve GraphQL endpoint (tests may override via `[sources.chirp]` JSON).
#[must_use]
pub fn resolve_graphql_url(source_config: &Value) -> String {
    source_config
        .get("graphql_url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_GRAPHQL_URL.to_string())
}

/// Canonical source id for handshake.
#[must_use]
pub fn source_id() -> &'static str {
    ID
}
