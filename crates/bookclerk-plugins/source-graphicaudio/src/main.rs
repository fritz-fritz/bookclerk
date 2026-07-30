//! External GraphicAudio source plugin for Bookclerk.

use bookclerk_plugin_sdk::{
    methods, BrandDto, FetchTitleParams, HandshakeResult, HealthDto, LoginParams, PluginGuest,
    ScanParams, PLUGIN_API_VERSION,
};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    PluginGuest::serve(|method, params| async move {
        match method.as_str() {
            methods::HANDSHAKE => Ok(serde_json::to_value(HandshakeResult {
                api_version: PLUGIN_API_VERSION,
                id: "graphicaudio".into(),
                kind: "source".into(),
                display_name: Some("GraphicAudio".into()),
                capabilities: vec![
                    "health".into(),
                    "diagnose".into(),
                    "login".into(),
                    "scan".into(),
                    "fetch_title".into(),
                ],
                portal_auth_mode: Some("password".into()),
                password_env_var: Some(
                    bookclerk_plugin_source_graphicaudio::GA_PASSWORD_ENV.into(),
                ),
                aliases: vec!["ga".into(), "graphic-audio".into()],
                sort_key: Some(2),
                brand: Some(BrandDto {
                    id: "graphicaudio".into(),
                    name: "GraphicAudio".into(),
                    bg: "#111827".into(),
                    fg: "#F9FAFB".into(),
                    accent: "#DC2626".into(),
                    icon_url: "https://www.google.com/s2/favicons?domain=graphicaudio.com&sz=128"
                        .into(),
                }),
                config_options: vec![],
                cli: None,
            })
            .unwrap()),
            methods::HEALTH => Ok(serde_json::to_value(HealthDto {
                id: "graphicaudio".into(),
                enabled: true,
                ok: true,
                detail: Some("graphicaudio source plugin ready".into()),
            })
            .unwrap()),
            methods::DIAGNOSE => Ok(json!(["graphicaudio plugin diagnose: ok"])),
            methods::LOGIN => {
                let p: LoginParams =
                    serde_json::from_value(params).map_err(|e| format!("login params: {e}"))?;
                let cfg = Value::Null;
                let access_url =
                    bookclerk_plugin_source_graphicaudio::resolve_access_base_url(&cfg);
                let store_url = bookclerk_plugin_source_graphicaudio::resolve_store_base_url(&cfg);
                let access = bookclerk_plugin_source_graphicaudio::resolve_access(&cfg);
                let dto = bookclerk_plugin_source_graphicaudio::guest_login_rpc(
                    &access_url,
                    &store_url,
                    access,
                    p,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(dto).unwrap())
            }
            methods::SCAN => {
                let p: ScanParams =
                    serde_json::from_value(params).map_err(|e| format!("scan params: {e}"))?;
                let cfg = Value::Null;
                let access_url =
                    bookclerk_plugin_source_graphicaudio::resolve_access_base_url(&cfg);
                let store_url = bookclerk_plugin_source_graphicaudio::resolve_store_base_url(&cfg);
                let access = bookclerk_plugin_source_graphicaudio::resolve_access(&cfg);
                let dto = bookclerk_plugin_source_graphicaudio::guest_scan_rpc(
                    &access_url,
                    &store_url,
                    access,
                    None,
                    &p,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(dto).unwrap())
            }
            methods::FETCH_TITLE => {
                let p: FetchTitleParams = serde_json::from_value(params)
                    .map_err(|e| format!("fetch_title params: {e}"))?;
                let access_url =
                    bookclerk_plugin_source_graphicaudio::resolve_access_base_url(&p.source_config);
                let store_url =
                    bookclerk_plugin_source_graphicaudio::resolve_store_base_url(&p.source_config);
                let access = bookclerk_plugin_source_graphicaudio::resolve_access(&p.source_config);
                let bitrate =
                    bookclerk_plugin_source_graphicaudio::resolve_bitrate(&p.source_config);
                let container =
                    bookclerk_plugin_source_graphicaudio::resolve_container(&p.source_config);
                let dto = bookclerk_plugin_source_graphicaudio::guest_fetch_title_rpc(
                    &access_url,
                    &store_url,
                    &p,
                    access,
                    bitrate,
                    container,
                    None,
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
