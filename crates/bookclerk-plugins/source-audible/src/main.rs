//! External Audible source plugin for Bookclerk.

use std::path::PathBuf;

use bookclerk_plugin_sdk::{
    methods, BrandDto, ConfigOptionDto, ConfigOptionValueDto, FetchTitleParams, HandshakeResult,
    HealthDto, LoginCompleteParams, LoginParams, LoginStartResultDto, PluginGuest, ScanParams,
    PLUGIN_API_VERSION,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    PluginGuest::serve(|method, params| async move {
        match method.as_str() {
            methods::HANDSHAKE => Ok(serde_json::to_value(HandshakeResult {
                api_version: PLUGIN_API_VERSION,
                id: "audible".into(),
                kind: "source".into(),
                display_name: Some("Audible".into()),
                capabilities: vec![
                    "health".into(),
                    "diagnose".into(),
                    "login.start".into(),
                    "login.complete".into(),
                    "scan".into(),
                    "fetch_title".into(),
                ],
                portal_auth_mode: Some("oauth".into()),
                password_env_var: None,
                aliases: vec![],
                sort_key: Some(0),
                brand: Some(BrandDto {
                    id: "audible".into(),
                    name: "Audible".into(),
                    bg: "#F8991D".into(),
                    fg: "#111111".into(),
                    accent: "#D97706".into(),
                    icon_url: "https://www.google.com/s2/favicons?domain=audible.com&sz=128".into(),
                }),
                config_options: vec![ConfigOptionDto {
                    key: "bitrate".into(),
                    label: "Bitrate".into(),
                    values: vec![
                        ConfigOptionValueDto {
                            id: "high".into(),
                            label: "High".into(),
                        },
                        ConfigOptionValueDto {
                            id: "normal".into(),
                            label: "Normal".into(),
                        },
                    ],
                }],
                cli: None,
            })
            .unwrap()),
            methods::HEALTH => Ok(serde_json::to_value(HealthDto {
                id: "audible".into(),
                enabled: true,
                ok: true,
                detail: Some("audible source plugin ready".into()),
            })
            .unwrap()),
            methods::DIAGNOSE => Ok(json!(["audible plugin diagnose: ok"])),
            methods::LOGIN_START => {
                let p: LoginParams = serde_json::from_value(params)
                    .map_err(|e| format!("login.start params: {e}"))?;
                let (session_id, url) = bookclerk_plugin_source_audible::guest_login_start(&p)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(LoginStartResultDto { session_id, url }).unwrap())
            }
            methods::LOGIN_COMPLETE => {
                let p: LoginCompleteParams = serde_json::from_value(params)
                    .map_err(|e| format!("login.complete params: {e}"))?;
                let result = bookclerk_plugin_source_audible::guest_login_complete(&p.session_id)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(result).unwrap())
            }
            methods::SCAN => {
                let p: ScanParams =
                    serde_json::from_value(params).map_err(|e| format!("scan params: {e}"))?;
                let summary = bookclerk_plugin_source_audible::guest_scan(
                    &p.credentials,
                    &p.accounts,
                    p.page_size,
                    p.import_episodes,
                    p.import_plus_titles,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(summary).unwrap())
            }
            methods::FETCH_TITLE => {
                let p: FetchTitleParams = serde_json::from_value(params)
                    .map_err(|e| format!("fetch_title params: {e}"))?;
                let creds = p
                    .credentials
                    .ok_or_else(|| "fetch_title requires host credentials".to_string())?;
                let dto = bookclerk_plugin_source_audible::guest_fetch_title(
                    &creds,
                    &p.title_id,
                    &PathBuf::from(p.cache_dir),
                    &p.source_config,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(dto).unwrap())
            }
            other => Err(format!("unsupported method `{other}`")),
        }
    })
    .await?;
    Ok(())
}
