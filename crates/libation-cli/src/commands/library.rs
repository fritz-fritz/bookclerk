//! `libation library` — scan, liberate, set-status, get-license.

use clap::Subcommand;
use libation_audible::DownloadOptions;
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
            let accounts = store.list_accounts()?;
            if accounts.is_empty() {
                anyhow::bail!("no accounts configured — run `libation auth login` first");
            }
            let targets: Vec<_> = accounts
                .into_iter()
                .filter(|a| account.as_ref().is_none_or(|id| id == &a.account_id))
                .collect();
            if targets.is_empty() {
                anyhow::bail!("no matching account for scan");
            }
            for acct in &targets {
                eprintln!(
                    "scan: account {} ({}) — audible-rs library sync pending",
                    acct.account_id, acct.marketplace
                );
            }
            eprintln!(
                "note: library scan will call audible-rs `library sync` and upsert into {}",
                paths.library_db.display()
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

            let options = DownloadOptions::from(&config.download);
            for book in targets {
                let req = LiberateRequest {
                    asin: book.asin.clone(),
                    account_id: book.account_id.clone(),
                    title: book.title.clone(),
                    authors: book.authors.clone(),
                    options: options.clone(),
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
            let mut updated = 0u32;
            for book in books {
                let Some(key) = &book.storage_key else {
                    continue;
                };
                let exists = storage.exists(key).await?;
                let new_status = if exists {
                    LiberateStatus::Liberated
                } else {
                    LiberateStatus::NotLiberated
                };
                if new_status != book.liberate_status {
                    store.set_liberate_status(
                        &book.asin,
                        &book.account_id,
                        new_status,
                        Some(key),
                        None,
                    )?;
                    updated += 1;
                    println!(
                        "{}\t{} -> {}",
                        book.asin,
                        book.liberate_status.as_str(),
                        new_status.as_str()
                    );
                }
            }
            eprintln!("updated {updated} book(s)");
            Ok(())
        }
        LibraryCommand::GetLicense { asin, account: _ } => {
            anyhow::bail!(
                "get-license for {asin} is not wired yet (audible-rs license API pending)"
            );
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
