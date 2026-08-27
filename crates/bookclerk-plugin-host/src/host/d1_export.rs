//! Cloudflare D1 REST SQL export for host schema snapshots.

use std::time::Duration;

use bookclerk_config::{resolve_d1_api_token, Config, DatabasePluginKind};
use serde_json::Value as JsonValue;
use tokio::task::spawn_blocking;

use crate::{PluginError, Result};

/// Cloudflare D1 identifiers used to snapshot the library database.
#[derive(Debug, Clone)]
pub struct D1SnapshotCreds {
    /// API origin with no trailing slash.
    pub api_base: String,
    /// Cloudflare account id.
    pub account_id: String,
    /// D1 database UUID.
    pub database_id: String,
}

impl D1SnapshotCreds {
    /// Builds snapshot credentials from `[database.d1]` when the active plugin is D1.
    #[must_use]
    pub fn from_config(config: &Config) -> Option<Self> {
        match DatabasePluginKind::parse(&config.database.plugin) {
            Some(DatabasePluginKind::D1) => Some(Self {
                api_base: config.database.d1.api_base.clone(),
                account_id: config.database.d1.account_id.clone(),
                database_id: config.database.d1.database_id.clone(),
            }),
            _ => None,
        }
    }
}

/// Exports the configured library D1 database as SQL via Cloudflare REST.
///
/// Time Travel is unused. Failures are returned to the caller so snapshots can
/// fall back to a connection dump.
///
/// # Errors
///
/// Returns when the token is missing or the export/poll/download fails.
pub async fn export_d1_sql_dump(config: &Config) -> Result<Vec<u8>> {
    let creds = D1SnapshotCreds::from_config(config)
        .ok_or_else(|| PluginError::message("D1 SQL export requires [database].plugin = \"d1\""))?;
    let token = resolve_d1_api_token().map_err(|err| PluginError::message(err.to_string()))?;
    export_d1_sql(
        &creds.api_base,
        &creds.account_id,
        &token,
        &creds.database_id,
    )
    .await
}

/// Best-effort D1 REST export; `None` when the plugin is not D1 or export fails.
pub async fn try_export_d1_sql_dump(config: &Config) -> Option<Vec<u8>> {
    match export_d1_sql_dump(config).await {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            tracing::warn!(%err, "d1 REST export unavailable; snapshot will dump through the guest");
            None
        }
    }
}

/// POST `/d1/database/{id}/export` and poll until `signed_url` is ready.
///
/// # Errors
///
/// Returns when HTTP start, poll, or download fails.
pub async fn export_d1_sql(
    api_base: &str,
    account_id: &str,
    api_token: &str,
    database_id: &str,
) -> Result<Vec<u8>> {
    let api_base = api_base.trim_end_matches('/').to_string();
    let account_id = account_id.to_string();
    let api_token = api_token.to_string();
    let database_id = database_id.to_string();
    spawn_blocking(move || export_d1_sql_blocking(&api_base, &account_id, &api_token, &database_id))
        .await
        .map_err(|err| PluginError::message(format!("d1 export join: {err}")))?
}

/// Blocking ureq implementation of the D1 export poll loop.
fn export_d1_sql_blocking(
    api_base: &str,
    account_id: &str,
    api_token: &str,
    database_id: &str,
) -> Result<Vec<u8>> {
    let url = format!("{api_base}/accounts/{account_id}/d1/database/{database_id}/export");
    let mut bookmark: Option<String> = None;
    let mut signed: Option<String> = None;
    for _ in 0..20 {
        let mut body = serde_json::json!({ "output_format": "polling" });
        if let Some(current) = bookmark.as_ref() {
            body["current_bookmark"] = serde_json::Value::String(current.clone());
        }
        let mut response = ureq::post(&url)
            .header("Authorization", format!("Bearer {api_token}"))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|err| PluginError::message(format!("d1 export: {err}")))?;
        if !response.status().is_success() {
            return Err(PluginError::message(format!(
                "d1 export HTTP {}",
                response.status()
            )));
        }
        let polled: JsonValue = response
            .body_mut()
            .read_json()
            .map_err(|err| PluginError::message(format!("d1 export json: {err}")))?;
        if polled.get("success").and_then(JsonValue::as_bool) == Some(false) {
            return Err(PluginError::message(format!(
                "d1 export rejected: {}",
                polled.get("errors").cloned().unwrap_or(JsonValue::Null)
            )));
        }
        signed = polled
            .pointer("/result/signed_url")
            .and_then(JsonValue::as_str)
            .map(str::to_string);
        bookmark = polled
            .pointer("/result/at_bookmark")
            .and_then(JsonValue::as_str)
            .map(str::to_string);
        let status = polled
            .pointer("/result/status")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        if status.eq_ignore_ascii_case("error") || status.eq_ignore_ascii_case("failed") {
            return Err(PluginError::message(format!("d1 export failed: {polled}")));
        }
        if signed.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let signed =
        signed.ok_or_else(|| PluginError::message("d1 export timed out waiting for signed_url"))?;
    let mut download = ureq::get(&signed)
        .call()
        .map_err(|err| PluginError::message(format!("d1 export download: {err}")))?;
    let bytes = download
        .body_mut()
        .read_to_vec()
        .map_err(|err| PluginError::message(format!("d1 export download: {err}")))?;
    Ok(bytes)
}
