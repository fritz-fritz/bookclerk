//! `libation library` — scan, liberate, set-status, get-license.

use clap::Subcommand;
use libation_audible::{
    open_account_client, request_content_license, summarize_license, DownloadOptions,
};
use libation_config::Config;
use libation_liberate::{
    liberate_book_indexed, reconcile_library, LiberateRequest, ReconcileOptions, StorageIndex,
};
use libation_library::{LiberateStatus, LibraryStore};
use libation_storage::from_config;

#[derive(Debug, Subcommand)]
pub enum LibraryCommand {
    /// Sync Audible library into the local DB (LibationCli: `scan`).
    Scan {
        /// Limit sync to one account id.
        #[arg(long)]
        account: Option<String>,
        /// After scan, match existing files in storage to library rows.
        #[arg(long)]
        match_storage: bool,
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
        /// Re-download even when matching media already exists in storage.
        #[arg(long)]
        force: bool,
    },
    /// Match storage files to library rows and update liberate status.
    ///
    /// Finds media by planned path (`Author/Title/ASIN.m4b`) or by ASIN
    /// appearing in the file path (classic Libation `Title [ASIN].m4b` layouts).
    SetStatus {
        #[arg(long)]
        account: Option<String>,
        /// Do not clear Liberated status when the file is missing.
        #[arg(long)]
        keep_missing: bool,
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
        LibraryCommand::Scan {
            account,
            match_storage,
        } => {
            let summary = libation_audible::scan_library(
                &paths.files_dir,
                &store,
                libation_audible::ScanOptions {
                    account: account.clone(),
                    page_size: 50,
                },
            )
            .await?;
            println!(
                "scan complete: {} account(s), {} book upsert(s), {} page(s)",
                summary.accounts, summary.books_upserted, summary.pages
            );
            if match_storage {
                let storage = from_config(config).await?;
                let recon = reconcile_library(
                    &store,
                    storage.as_ref(),
                    ReconcileOptions {
                        account,
                        clear_missing: true,
                    },
                )
                .await?;
                println!(
                    "storage match: matched={} cleared={} unchanged={}",
                    recon.matched, recon.cleared, recon.unchanged
                );
            }
            Ok(())
        }
        LibraryCommand::Liberate {
            asin,
            account,
            dry_run,
            force,
        } => {
            let storage = from_config(config).await?;
            let books = store.list_books(account.as_deref())?;
            let targets: Vec<_> = books
                .into_iter()
                .filter(|b| asin.as_ref().is_none_or(|a| a == &b.asin))
                .filter(|b| force || b.liberate_status != LiberateStatus::Liberated)
                .collect();

            if targets.is_empty() {
                eprintln!("nothing to liberate");
                return Ok(());
            }

            paths.ensure_dirs()?;
            let options = DownloadOptions::from(&config.download);
            let index = if dry_run {
                None
            } else {
                Some(StorageIndex::from_storage(storage.as_ref()).await?)
            };

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
                    force,
                };
                if dry_run {
                    let key = libation_liberate::planned_storage_key(&req);
                    println!("{}\t{}", book.asin, key);
                    continue;
                }
                match liberate_book_indexed(
                    &store,
                    storage.as_ref(),
                    req,
                    index.as_ref(),
                )
                .await
                {
                    Ok(result) if result.matched_existing => {
                        println!("matched {} -> {}", result.asin, result.storage_key);
                    }
                    Ok(result) => {
                        println!("liberated {} -> {}", result.asin, result.storage_key);
                    }
                    Err(err) => eprintln!("liberate {}: {err}", book.asin),
                }
            }
            Ok(())
        }
        LibraryCommand::SetStatus {
            account,
            keep_missing,
        } => {
            let storage = from_config(config).await?;
            let summary = reconcile_library(
                &store,
                storage.as_ref(),
                ReconcileOptions {
                    account,
                    clear_missing: !keep_missing,
                },
            )
            .await?;
            println!(
                "matched {}\tcleared {}\tunchanged {}",
                summary.matched, summary.cleared, summary.unchanged
            );
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
