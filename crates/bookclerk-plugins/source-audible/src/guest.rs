//! Guest-process helpers for the external Audible source plugin.
//!
//! Credentials are opaque JSON (`authfile_b64`, optional `widevine_b64`). The
//! host seals them; this module never opens the library DB.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use audible_rs::api::client::Client;
use audible_rs::auth::login::{self as login_flow, LoginServer};
use audible_rs::auth::Authenticator;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use bookclerk_config::AudioQuality;
use crate::drm::{decrypt_adrm, decrypt_cenc, CencDecryptRequest, DecryptRequest};
use bookclerk_plugin_sdk::{
    LoginParams, LoginResultDto, PlainPartDto, ScanBookDto, ScanSummaryDto, SourceAccountDto,
    SourceFetchDto,
};
use serde_json::{json, Value};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::artifacts::{download_cover_jpeg, fetch_chapter_info};
use crate::auth::{
    export_authfile_plain_bytes, login_defaults, login_server_url, session_from_authenticator,
    AuthLoginOptions,
};
use crate::download::{fetch_and_download_with_client, AccountClient, DrmKind};
use crate::error::{AudibleError, Result};
use crate::options::DownloadOptions;
use crate::source::ID;
use crate::sync::collect_account_books;

struct PendingGuestLogin {
    label: Option<String>,
    handle: JoinHandle<Result<Authenticator>>,
}

fn pending_logins() -> &'static Mutex<HashMap<String, PendingGuestLogin>> {
    static PENDING: OnceLock<Mutex<HashMap<String, PendingGuestLogin>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start LoginServer OAuth; return `(session_id, browser_url)`.
///
/// Completes later via [`guest_login_complete`].
pub async fn guest_login_start(params: &LoginParams) -> Result<(String, String)> {
    let marketplace = if params.marketplace.trim().is_empty() {
        String::from("us")
    } else {
        params.marketplace.trim().to_ascii_lowercase()
    };
    let callback_bind = parse_callback_bind(params.callback_bind.as_deref())?;
    let opts = AuthLoginOptions {
        marketplace,
        label: params.label.clone(),
        callback_bind,
        show_qr: false,
        scope: None,
        force: params.force,
        ..AuthLoginOptions::default()
    };

    let defaults = login_defaults(&opts);
    let server = LoginServer::bind(opts.callback_bind, defaults).await?;
    let (url, _addr) = login_server_url(&server, opts.callback_bind);
    let timeout = Duration::from_secs(opts.timeout_secs);
    let label = opts.label.clone();

    let handle = tokio::spawn(async move {
        let login = server.run(timeout).await?;
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|err| AudibleError::Auth(err.to_string()))?;
        let auth = login_flow::register(
            &http,
            &login.locale,
            &login.device,
            &login.pkce,
            &login.code,
            login.with_username,
        )
        .await?;
        Ok(auth)
    });

    let session_id = Uuid::new_v4().to_string();
    pending_logins()
        .lock()
        .map_err(|_| AudibleError::Auth("guest login session lock poisoned".into()))?
        .insert(session_id.clone(), PendingGuestLogin { label, handle });

    Ok((session_id, url))
}

/// Await a pending OAuth session and return account + credential JSON.
pub async fn guest_login_complete(session_id: &str) -> Result<LoginResultDto> {
    let pending = pending_logins()
        .lock()
        .map_err(|_| AudibleError::Auth("guest login session lock poisoned".into()))?
        .remove(session_id)
        .ok_or_else(|| {
            AudibleError::Auth(format!(
                "unknown or expired guest login session `{session_id}`"
            ))
        })?;

    let auth = pending
        .handle
        .await
        .map_err(|e| AudibleError::Auth(format!("guest login task failed: {e}")))??;

    let session = session_from_authenticator(&auth, pending.label);
    let credentials = credentials_json_from_auth(&auth, None)?;

    Ok(LoginResultDto {
        account: SourceAccountDto {
            account_id: session.account_id,
            source: ID.into(),
            marketplace: session.marketplace,
            label: session.label,
            scan_enabled: true,
        },
        credentials: Some(credentials),
    })
}

/// Scan libraries for host-injected credential blobs.
pub async fn guest_scan(
    credentials: &BTreeMap<String, Value>,
    account_filter: &[String],
    page_size: u32,
    import_episodes: bool,
    import_plus_titles: bool,
) -> Result<ScanSummaryDto> {
    if credentials.is_empty() {
        return Err(AudibleError::NoAccounts(
            "no Audible credentials from host — run login first".into(),
        ));
    }
    let explicit = !account_filter.is_empty();
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
        let auth = authenticator_from_credentials(creds)?;
        let marketplace = auth.locale().country_code.to_string();
        let resolved_id = auth
            .customer_id()
            .map(str::to_string)
            .unwrap_or_else(|| account_id.clone());
        let client = Client::new(auth).map_err(AudibleError::from)?;
        let (batch, p) = collect_account_books(
            &client,
            &resolved_id,
            &marketplace,
            page_size,
            import_episodes,
            import_plus_titles,
        )
        .await?;
        pages = pages.saturating_add(p);
        accounts += 1;
        books.extend(batch.into_iter().map(new_book_to_scan));
    }

    if accounts == 0 {
        return Err(AudibleError::NoAccounts(
            "no matching Audible accounts in host credentials".into(),
        ));
    }
    let n = books.len();
    Ok(ScanSummaryDto {
        accounts,
        books_upserted: n,
        pages,
        skipped_disabled: 0,
        books,
    })
}

/// Download + decrypt one title; return plain paths (never encrypted on the wire).
pub async fn guest_fetch_title(
    credentials: &Value,
    title_id: &str,
    cache_dir: &Path,
    source_config: &Value,
) -> Result<SourceFetchDto> {
    let auth = authenticator_from_credentials(credentials)?;
    let marketplace = auth.locale().country_code.to_string();
    let account_id = auth
        .customer_id()
        .map(str::to_string)
        .unwrap_or_else(|| "audible".into());
    let client = Client::new(auth).map_err(AudibleError::from)?;
    let account_client = AccountClient {
        client: std::sync::Arc::new(client),
        account_id,
        marketplace: marketplace.clone(),
    };

    // Optional BYO Widevine CDM from credential blob → cache_dir/widevine.wvd.
    if let Some(wvd_b64) = credentials.get("widevine_b64").and_then(Value::as_str) {
        match STANDARD.decode(wvd_b64) {
            Ok(bytes) => {
                let dest = cache_dir.join("widevine.wvd");
                if let Err(err) = tokio::fs::write(&dest, &bytes).await {
                    tracing::warn!(error = %err, "failed to write guest widevine.wvd");
                }
            }
            Err(err) => tracing::warn!(error = %err, "invalid widevine_b64 in credentials"),
        }
    }

    let quality = resolve_bitrate(source_config);
    let options = DownloadOptions {
        quality,
        ..DownloadOptions::default()
    };

    let (account, download, _summary) = fetch_and_download_with_client(
        account_client,
        cache_dir,
        title_id,
        &options,
        cache_dir,
        None,
    )
    .await?;

    let work_dir = cache_dir.join(title_id);
    tokio::fs::create_dir_all(&work_dir).await?;
    let m4b_path = work_dir.join(format!("{title_id}.m4b"));

    let plain_path = if download.needs_decrypt {
        match download.drm_kind {
            DrmKind::Adrm => {
                let key = download.key.clone().ok_or_else(|| {
                    AudibleError::License(format!("{title_id}: Adrm download missing key"))
                })?;
                let iv = download.iv.clone().ok_or_else(|| {
                    AudibleError::License(format!("{title_id}: Adrm download missing iv"))
                })?;
                let outcome = decrypt_adrm(DecryptRequest {
                    input: download.path.clone(),
                    output: m4b_path.clone(),
                    audible_key: Some(key),
                    audible_iv: Some(iv),
                    activation_bytes: None,
                    trim: None,
                })
                .await
                .map_err(|e| AudibleError::Other(anyhow::anyhow!("decrypt Adrm: {e}")))?;
                outcome.output
            }
            DrmKind::Widevine => {
                let kid = download.kid.clone().ok_or_else(|| {
                    AudibleError::Widevine(format!("{title_id}: Widevine download missing kid"))
                })?;
                let key = download.cenc_key.clone().ok_or_else(|| {
                    AudibleError::Widevine(format!("{title_id}: Widevine download missing key"))
                })?;
                let outcome = decrypt_cenc(CencDecryptRequest {
                    input: download.path.clone(),
                    output: m4b_path.clone(),
                    kid,
                    key,
                    trim: None,
                })
                .await
                .map_err(|e| AudibleError::Other(anyhow::anyhow!("decrypt CENC: {e}")))?;
                outcome.output
            }
            DrmKind::Mpeg => download.path.clone(),
        }
    } else {
        // Plain Mpeg / already-clear media — copy or reuse path.
        if download.path != m4b_path {
            if let Some(parent) = m4b_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::copy(&download.path, &m4b_path).await?;
            m4b_path
        } else {
            download.path
        }
    };

    let mut cover_path = None;
    let cover_dest = work_dir.join(format!("{title_id}.cover.jpg"));
    match download_cover_jpeg(
        &account.client,
        &account.marketplace,
        title_id,
        &options.cover_size,
        &cover_dest,
    )
    .await
    {
        Ok(path) => cover_path = path,
        Err(err) => {
            tracing::warn!(asin = %title_id, error = %err, "guest cover download failed");
        }
    }

    let mut chapters = Vec::new();
    match fetch_chapter_info(
        &account.client,
        &account.marketplace,
        title_id,
        quality,
        "Tree",
    )
    .await
    {
        Ok(info) => chapters = flatten_chapters(&info),
        Err(err) => {
            tracing::warn!(asin = %title_id, error = %err, "guest chapter fetch failed");
        }
    }

    Ok(SourceFetchDto::Plain {
        parts: vec![PlainPartDto {
            path: plain_path.display().to_string(),
            title: None,
            duration_ms: None,
        }],
        m4b_path: Some(plain_path.display().to_string()),
        cover_path: cover_path.map(|p| p.display().to_string()),
        chapters,
    })
}

/// Build credential JSON from an authenticator (optional Widevine CDM bytes).
pub fn credentials_json_from_auth(auth: &Authenticator, widevine: Option<&[u8]>) -> Result<Value> {
    let plain = export_authfile_plain_bytes(auth)?;
    let mut obj = serde_json::Map::new();
    obj.insert("authfile_b64".into(), json!(STANDARD.encode(plain)));
    if let Some(wvd) = widevine {
        obj.insert("widevine_b64".into(), json!(STANDARD.encode(wvd)));
    }
    Ok(Value::Object(obj))
}

fn authenticator_from_credentials(creds: &Value) -> Result<Authenticator> {
    let b64 = creds
        .get("authfile_b64")
        .and_then(Value::as_str)
        .ok_or_else(|| AudibleError::Auth("credentials missing authfile_b64".into()))?;
    let bytes = STANDARD
        .decode(b64)
        .map_err(|e| AudibleError::Auth(format!("invalid authfile_b64: {e}")))?;
    Authenticator::load_from_bytes(&bytes, None)
        .map_err(|e| AudibleError::Auth(format!("failed to decode audible auth: {e}")))
}

fn parse_callback_bind(raw: Option<&str>) -> Result<SocketAddr> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok("127.0.0.1:0".parse().expect("valid socket addr")),
        Some(s) => s
            .parse()
            .map_err(|e| AudibleError::Auth(format!("invalid callback_bind `{s}`: {e}"))),
    }
}

fn resolve_bitrate(source_config: &Value) -> AudioQuality {
    source_config
        .get("bitrate")
        .and_then(Value::as_str)
        .and_then(|raw| match raw.trim().to_ascii_lowercase().as_str() {
            "high" => Some(AudioQuality::High),
            "normal" => Some(AudioQuality::Normal),
            _ => None,
        })
        .unwrap_or_default()
}

fn new_book_to_scan(book: bookclerk_library::NewBook) -> ScanBookDto {
    ScanBookDto {
        account_id: book.account_id,
        product_id: book.product_id,
        title: book.title,
        marketplace: Some(book.marketplace),
        asin: book.asin,
        isbn: book.isbn,
        authors: book.authors,
        narrators: book.narrators,
        series: book.series,
        series_index: book.series_index,
        content_kind: Some(book.content_kind),
        publisher: book.publisher,
        length_minutes: book.length_minutes,
        subtitle: book.subtitle,
    }
}

fn flatten_chapters(info: &Value) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    if let Some(arr) = info.get("chapters").and_then(Value::as_array) {
        flatten_chapter_nodes(arr, &mut out);
    }
    out.sort_by_key(|(_, start)| *start);
    out.dedup_by_key(|(_, start)| *start);
    out
}

fn flatten_chapter_nodes(nodes: &[Value], out: &mut Vec<(String, u64)>) {
    for node in nodes {
        if let Some(nested) = node.get("chapters").and_then(Value::as_array) {
            flatten_chapter_nodes(nested, out);
        }
        let Some(title) = node.get("title").and_then(Value::as_str) else {
            continue;
        };
        let start_ms = node
            .get("start_offset_ms")
            .and_then(Value::as_u64)
            .or_else(|| {
                node.get("start_offset_ms")
                    .and_then(Value::as_i64)
                    .filter(|v| *v >= 0)
                    .map(|v| v as u64)
            })
            .or_else(|| node.get("startOffsetMs").and_then(Value::as_u64))
            .unwrap_or(0);
        if !title.trim().is_empty() {
            out.push((title.trim().to_string(), start_ms));
        }
    }
}
