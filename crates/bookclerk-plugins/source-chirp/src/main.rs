//! External Chirp source plugin for Bookclerk.

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
                id: "chirp".into(),
                kind: "source".into(),
                display_name: Some("Chirp".into()),
                capabilities: vec![
                    "health".into(),
                    "diagnose".into(),
                    "login".into(),
                    "scan".into(),
                    "fetch_title".into(),
                ],
                portal_auth_mode: Some("password".into()),
                password_env_var: Some(bookclerk_plugin_source_chirp::PASSWORD_ENV.into()),
                aliases: vec![],
                sort_key: Some(3),
                brand: Some(BrandDto {
                    id: "chirp".into(),
                    name: "Chirp".into(),
                    bg: "#E85D04".into(),
                    fg: "#FFFFFF".into(),
                    accent: "#F48C06".into(),
                    icon_url: "https://www.google.com/s2/favicons?domain=chirpbooks.com&sz=128"
                        .into(),
                }),
                config_options: vec![],
                cli: None,
            })
            .unwrap()),
            methods::HEALTH => Ok(serde_json::to_value(HealthDto {
                id: "chirp".into(),
                enabled: true,
                ok: true,
                detail: Some("chirp source plugin ready".into()),
            })
            .unwrap()),
            methods::DIAGNOSE => Ok(json!(["chirp plugin diagnose: ok"])),
            methods::LOGIN => {
                let p: LoginParams =
                    serde_json::from_value(params).map_err(|e| format!("login params: {e}"))?;
                let gql = bookclerk_plugin_source_chirp::resolve_graphql_url(&Value::Null);
                let dto = bookclerk_plugin_source_chirp::guest_login_rpc(&gql, p)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(dto).unwrap())
            }
            methods::SCAN => {
                let p: ScanParams =
                    serde_json::from_value(params).map_err(|e| format!("scan params: {e}"))?;
                let gql = bookclerk_plugin_source_chirp::resolve_graphql_url(&Value::Null);
                let dto = bookclerk_plugin_source_chirp::guest_scan_rpc(&gql, &p)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(dto).unwrap())
            }
            methods::FETCH_TITLE => {
                let p: FetchTitleParams = serde_json::from_value(params)
                    .map_err(|e| format!("fetch_title params: {e}"))?;
                let gql = bookclerk_plugin_source_chirp::resolve_graphql_url(&p.source_config);
                let dto = bookclerk_plugin_source_chirp::guest_fetch_title_rpc(&gql, &p)
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
