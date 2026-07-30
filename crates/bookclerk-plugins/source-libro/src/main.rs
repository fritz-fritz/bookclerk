//! External Libro.fm source plugin for Bookclerk.

use std::path::PathBuf;

use bookclerk_library::NewBook;
use bookclerk_plugin_sdk::{
    methods, BrandDto, ConfigOptionDto, ConfigOptionValueDto, FetchTitleParams, HandshakeResult,
    HealthDto, LoginParams, LoginResultDto, PlainPartDto, PluginGuest, ScanBookDto, ScanParams,
    ScanSummaryDto, SourceAccountDto, SourceFetchDto, PLUGIN_API_VERSION,
};
use bookclerk_source::LoginOptions;
use serde_json::{json, Value};

fn new_book_to_scan(book: NewBook) -> ScanBookDto {
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

fn plain_to_dto(plain: bookclerk_source::PlainFetch) -> SourceFetchDto {
    SourceFetchDto::Plain {
        parts: plain
            .parts
            .into_iter()
            .map(|p| PlainPartDto {
                path: p.path.display().to_string(),
                title: p.title,
                duration_ms: p.duration_ms,
            })
            .collect(),
        m4b_path: plain.m4b_path.map(|p| p.display().to_string()),
        cover_path: plain.cover_path.map(|p| p.display().to_string()),
        chapters: plain.chapters,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    PluginGuest::serve(|method, params| async move {
        match method.as_str() {
            methods::HANDSHAKE => Ok(serde_json::to_value(HandshakeResult {
                api_version: PLUGIN_API_VERSION,
                id: "libro".into(),
                kind: "source".into(),
                display_name: Some("Libro.fm".into()),
                capabilities: vec![
                    "health".into(),
                    "diagnose".into(),
                    "login".into(),
                    "scan".into(),
                    "fetch_title".into(),
                ],
                portal_auth_mode: Some("password".into()),
                password_env_var: Some(bookclerk_libro::PASSWORD_ENV.into()),
                aliases: vec!["libro.fm".into(), "librofm".into()],
                sort_key: Some(1),
                brand: Some(BrandDto {
                    id: "libro".into(),
                    name: "Libro.fm".into(),
                    bg: "#1F4E3D".into(),
                    fg: "#F4F1EA".into(),
                    accent: "#2F6B53".into(),
                    icon_url: "https://www.google.com/s2/favicons?domain=libro.fm&sz=128".into(),
                }),
                config_options: vec![ConfigOptionDto {
                    key: "container".into(),
                    label: "Container".into(),
                    values: vec![
                        ConfigOptionValueDto {
                            id: "m4b".into(),
                            label: "M4B".into(),
                        },
                        ConfigOptionValueDto {
                            id: "zip".into(),
                            label: "ZIP (MP3 parts)".into(),
                        },
                    ],
                }],
                cli: None,
            })
            .unwrap()),
            methods::HEALTH => Ok(serde_json::to_value(HealthDto {
                id: "libro".into(),
                enabled: true,
                ok: true,
                detail: Some("libro source plugin ready".into()),
            })
            .unwrap()),
            methods::DIAGNOSE => Ok(json!(["libro plugin diagnose: ok"])),
            methods::LOGIN => {
                let p: LoginParams =
                    serde_json::from_value(params).map_err(|e| format!("login params: {e}"))?;
                let base = bookclerk_libro::resolve_base_url(&Value::Null);
                let (account_id, marketplace, label, scan_enabled, credentials) =
                    bookclerk_libro::guest_login(
                        &base,
                        LoginOptions {
                            marketplace: p.marketplace,
                            label: p.label,
                            email: p.email,
                            password: p.password,
                            force: p.force,
                        },
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(LoginResultDto {
                    account: SourceAccountDto {
                        account_id,
                        source: "libro".into(),
                        marketplace,
                        label,
                        scan_enabled,
                    },
                    credentials: Some(credentials),
                })
                .unwrap())
            }
            methods::SCAN => {
                let p: ScanParams =
                    serde_json::from_value(params).map_err(|e| format!("scan params: {e}"))?;
                let base = bookclerk_libro::resolve_base_url(&Value::Null);
                let (books, accounts, pages) =
                    bookclerk_libro::guest_scan(&base, &p.credentials, &p.accounts)
                        .await
                        .map_err(|e| e.to_string())?;
                let n = books.len();
                Ok(serde_json::to_value(ScanSummaryDto {
                    accounts,
                    books_upserted: n,
                    pages,
                    skipped_disabled: 0,
                    books: books.into_iter().map(new_book_to_scan).collect(),
                })
                .unwrap())
            }
            methods::FETCH_TITLE => {
                let p: FetchTitleParams = serde_json::from_value(params)
                    .map_err(|e| format!("fetch_title params: {e}"))?;
                let creds = p
                    .credentials
                    .ok_or_else(|| "fetch_title requires host credentials".to_string())?;
                let base = bookclerk_libro::resolve_base_url(&p.source_config);
                let container = bookclerk_libro::resolve_container(&p.source_config);
                let plain = bookclerk_libro::guest_fetch_title(
                    &base,
                    &creds,
                    &p.title_id,
                    &PathBuf::from(p.cache_dir),
                    container,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(plain_to_dto(plain)).unwrap())
            }
            other => Err(format!("unsupported method `{other}`")),
        }
    })
    .await?;
    Ok(())
}
