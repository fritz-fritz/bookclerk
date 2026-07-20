//! `libation library` — scan, liberate, set-status, get-license.

use clap::Subcommand;
use libation_audible::{
    open_account_client, request_content_license, summarize_license, DownloadOptions,
};
use libation_config::Config;
use libation_liberate::{liberate_book, LiberateRequest};
use libation_library::{LiberateStatus, LibraryStore};
use libation_storage::from_config;

#[derive(Debug, Subcommand)]
pub enum LibraryCommand {
    /// Sync Audible library into the local DB (LibationCli: `scan`).
    Scan {
        /// Limit sync to one account id.
        #[arg(long)]
        account: Option<String>,
    },
    /// Download + decrypt + store titles (LibationCli: `liberate`).
    Liberate {
        /// Liberate a single ASIN.
        #[arg(long)]
        asin: Option<String>,
        /// Account id (required with `--asin` when multiple accounts exist).
        #[arg(long)]
        account: Option<String>,
        /// Dry-run: print planned storage keys only.
        #[arg(long)]
        dry_run: bool,
    },
    /// Reconcile DB liberate state against storage (LibationCli: `set-status`).
    SetStatus {
        #[arg(long)]
        account: Option<String>,
    },
    /// Fetch a content license for an ASIN (LibationCli: `get-license`).
    GetLicense {
        asin: String,
        #[arg(long)]
        account: Option<String>,
    },
    /// List books in the local library DB.
    List {
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
}

pub async fn run(command: LibraryCommand, config: &Config) -> anyhow::Result<()> {
    let paths = config.paths();
    let store = LibraryStore::open(&paths.library_db)?;

    match command {
        LibraryCommand::Scan { account } => {
            let summary = libation_audible::scan_library(
                &paths.files_dir,
                &store,
                libation_audible::ScanOptions {
                    account,
                    page_size: 50,
                },
            )
            .await?;
            println!(
                "scan complete: {} account(s), {} book upsert(s), {} page(s)",
                summary.accounts, summary.books_upserted, summary.pages
            );
            Ok(())
        }
        LibraryCommand::Liberate {
            asin,
            account,
            dry_run,
        } => {
            let storage = from_config(config).await?;
            let books = store.list_books(account.as_deref())?;
            let targets: Vec<_> = books
                .into_iter()
                .filter(|b| asin.as_ref().is_none_or(|a| a == &b.asin))
                .filter(|b| b.liberate_status != LiberateStatus::Liberated)
                .collect();

            if targets.is_empty() {
                eprintln!("nothing to liberate");
                return Ok(());
            }

            paths.ensure_dirs()?;
            let options = DownloadOptions::from(&config.download);
            for book in targets {
                let req = LiberateRequest {
                    asin: book.asin.clone(),
                    account_id: book.account_id.clone(),
                    title: book.title.clone(),
                    authors: book.authors.clone(),
                    options: options.clone(),
                    files_dir: paths.files_dir.clone(),
                    cache_dir: paths.cache_dir.clone(),
                    aaxclean_bin: None,
                };
                if dry_run {
                    let key = libation_liberate::planned_storage_key(&req);
                    println!("{}\t{}", book.asin, key);
                    continue;
                }
                match liberate_book(&store, storage.as_ref(), req).await {
                    Ok(result) => println!("liberated {} -> {}", result.asin, result.storage_key),
                    Err(err) => eprintln!("liberate {}: {err}", book.asin),
                }
            }
            Ok(())
        }
        LibraryCommand::SetStatus { account } => {
            let storage = from_config(config).await?;
            let books = store.list_books(account.as_deref())?;
            let options = DownloadOptions::from(&config.download);
            let mut updated = 0u32;
            for book in books {
                let req = LiberateRequest {
                    asin: book.asin.clone(),
                    account_id: book.account_id.clone(),
                    title: book.title.clone(),
                    authors: book.authors.clone(),
                    options: options.clone(),
                    files_dir: paths.files_dir.clone(),
                    cache_dir: paths.cache_dir.clone(),
                    aaxclean_bin: None,
                };
                let key = book
                    .storage_key
                    .clone()
                    .unwrap_or_else(|| libation_liberate::planned_storage_key(&req));
                let exists = storage.exists(&key).await?;
                let new_status = if exists {
                    LiberateStatus::Liberated
                } else if book.liberate_status == LiberateStatus::Liberated {
                    LiberateStatus::NotLiberated
                } else {
                    book.liberate_status
                };
                if new_status != book.liberate_status
                    || (exists && book.storage_key.as_deref() != Some(key.as_str()))
                {
                    store.set_liberate_status(
                        &book.asin,
                        &book.account_id,
                        new_status,
                        if exists { Some(&key) } else { book.storage_key.as_deref() },
                        None,
                    )?;
                    updated += 1;
                    println!(
                        "{}\t{} -> {}\t{}",
                        book.asin,
                        book.liberate_status.as_str(),
                        new_status.as_str(),
                        key
                    );
                }
            }
            eprintln!("updated {updated} book(s)");
            Ok(())
        }
        LibraryCommand::GetLicense { asin, account } => {
            let account_key = resolve_account_for_asin(&store, &asin, account.as_deref())?;
            let client = open_account_client(&paths.files_dir, &account_key).await?;
            let license = request_content_license(
                &client.client,
                &client.marketplace,
                &asin,
                config.download.quality,
            )
            .await?;
            let summary = summarize_license(&license);
            println!("asin\t{}", summary.asin);
            println!("status\t{}", summary.status_code);
            println!(
                "drm_type\t{}",
                summary.drm_type.as_deref().unwrap_or("-")
            );
            println!(
                "content_format\t{}",
                summary.content_format.as_deref().unwrap_or("-")
            );
            println!(
                "content_size\t{}",
                summary
                    .content_size
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "-".into())
            );
            println!("granted\t{}", summary.granted);
            println!("has_voucher\t{}", summary.has_voucher);
            println!("offline_url\t{}", summary.offline_url_present);
            if let Some(msg) = &summary.denial_message {
                println!("denial\t{msg}");
            }
            Ok(())
        }
        LibraryCommand::List { account, status } => {
            let books = store.list_books(account.as_deref())?;
            for book in books {
                if let Some(filter) = &status {
                    if book.liberate_status.as_str() != filter.as_str() {
                        continue;
                    }
                }
                println!(
                    "{}\t{}\t{}\t{}",
                    book.asin,
                    book.liberate_status.as_str(),
                    book.title,
                    book.storage_key.as_deref().unwrap_or("-")
                );
            }
            Ok(())
        }
    }
}

fn resolve_account_for_asin(
    store: &LibraryStore,
    asin: &str,
    account: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(account) = account {
        return Ok(account.to_string());
    }
    let books = store.list_books(None)?;
    let matches: Vec<_> = books.into_iter().filter(|b| b.asin == asin).collect();
    match matches.as_slice() {
        [] => anyhow::bail!("ASIN {asin} not in library — pass --account or run library scan"),
        [one] => Ok(one.account_id.clone()),
        many => anyhow::bail!(
            "ASIN {asin} exists on {} accounts; pass --account ({})",
            many.len(),
            many.iter()
                .map(|b| b.account_id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
