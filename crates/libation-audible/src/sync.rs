//! Library scan: fetch Audible library via audible-rs and upsert into libation-library.

use std::path::Path;

use audible_rs::api::client::Client;
use audible_rs::api::paginator;
use audible_rs::library_sync::DEFAULT_RESPONSE_GROUPS;
use audible_rs::models::library as lib_model;
use futures::TryStreamExt;
use libation_library::{LibraryStore, NewBook};
use reqwest::Method;

use crate::accounts::resolve_auth_file_async;
use crate::auth::load_authenticator;
use crate::error::{AudibleError, Result};
use crate::paths::list_auth_files;

/// Options for a library scan.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Limit to one account name / id (auth file stem or customer id).
    pub account: Option<String>,
    pub page_size: u32,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            account: None,
            page_size: 50,
        }
    }
}

/// Summary of a scan run.
#[derive(Debug, Clone, Default)]
pub struct ScanSummary {
    pub accounts: usize,
    pub books_upserted: usize,
    pub pages: u32,
}

/// Sync Audible library for configured accounts into `library`.
pub async fn scan_library(
    files_dir: &Path,
    library: &LibraryStore,
    options: ScanOptions,
) -> Result<ScanSummary> {
    let targets = resolve_targets(files_dir, options.account.as_deref()).await?;
    if targets.is_empty() {
        return Err(AudibleError::Auth(
            "no accounts configured — run `libation auth login` first".into(),
        ));
    }

    let mut summary = ScanSummary {
        accounts: targets.len(),
        ..Default::default()
    };

    for (account_key, auth_path) in targets {
        let auth = load_authenticator(&auth_path, None).await?;
        let marketplace = auth.locale().country_code.to_string();
        let account_id = auth
            .customer_id()
            .map(str::to_string)
            .unwrap_or_else(|| account_key.clone());

        library.upsert_account(
            &account_id,
            &marketplace,
            Some(&account_key),
            true,
        )?;

        let client = Client::new(auth).map_err(AudibleError::from)?;
        let page_size = options.page_size.to_string();
        let marketplace_q = marketplace.clone();

        let stream = paginator::pages(|_continuation| {
            client
                .request(Method::GET, "/1.0/library")
                .country_code(&marketplace_q)
                .query("response_groups", DEFAULT_RESPONSE_GROUPS)
                .query("num_results", &page_size)
                .query("image_sizes", "500,1215")
                .query("include_pending", "true")
                .query("status", "Active")
        });
        futures::pin_mut!(stream);

        while let Some(page) = stream.try_next().await.map_err(AudibleError::from)? {
            summary.pages += 1;
            for item in lib_model::normalize_items(&page.body) {
                if lib_model::should_soft_delete(&item) {
                    continue;
                }
                let Some(asin) = item.get("asin").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some(title) = lib_model::build_full_title(&item) else {
                    continue;
                };
                let series = lib_model::extract_series(&item)
                    .into_iter()
                    .next()
                    .map(|s| s.title);
                let series_index = lib_model::extract_series(&item)
                    .into_iter()
                    .next()
                    .and_then(|s| s.sequence);

                library.upsert_book(&NewBook {
                    asin: asin.to_string(),
                    account_id: account_id.clone(),
                    marketplace: marketplace.clone(),
                    title,
                    authors: join_named_people(&item, "authors"),
                    narrators: join_named_people(&item, "narrators"),
                    series,
                    series_index,
                    purchased_at: None,
                })?;
                summary.books_upserted += 1;
            }
        }

        tracing::info!(
            account = %account_id,
            marketplace = %marketplace,
            "library scan finished for account"
        );
    }

    Ok(summary)
}

async fn resolve_targets(
    files_dir: &Path,
    account: Option<&str>,
) -> Result<Vec<(String, std::path::PathBuf)>> {
    if let Some(account) = account {
        let path = resolve_auth_file_async(files_dir, account).await?;
        let key = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(account)
            .to_string();
        return Ok(vec![(key, path)]);
    }
    let mut out = Vec::new();
    for path in list_auth_files(files_dir)? {
        let key = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("account")
            .to_string();
        out.push((key, path));
    }
    Ok(out)
}

fn join_named_people(item: &serde_json::Value, field: &str) -> Option<String> {
    let arr = item.get(field)?.as_array()?;
    let names: Vec<&str> = arr
        .iter()
        .filter_map(|entry| entry.get("name").and_then(|v| v.as_str()))
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}
