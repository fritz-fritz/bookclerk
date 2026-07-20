//! `libation library` — scan, liberate, set-status, get-license, search, export.

use clap::Subcommand;
use libation_audible::{
    open_account_client, request_content_license, summarize_license, DownloadOptions,
};
use libation_config::Config;
use libation_liberate::{
    liberate_book_indexed, liberate_pdf_only, reconcile_library, LiberateRequest, ReconcileOptions,
    StorageIndex,
};
use libation_library::{LiberateStatus, LibraryStore};
use libation_search::SearchEngine;
use libation_storage::from_config;

use crate::commands::export::{export_csv, export_json, export_xlsx, filter_books, load_books};

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
        /// Positional ASIN(s) (classic `liberate B00X B00Y`).
        #[arg(value_name = "ASIN")]
        asins: Vec<String>,
        /// Account id (required with `--asin` when multiple accounts exist).
        #[arg(long)]
        account: Option<String>,
        /// Dry-run: print planned storage keys only.
        #[arg(long)]
        dry_run: bool,
        /// Re-download even when matching media already exists in storage.
        #[arg(short, long)]
        force: bool,
        /// Download companion PDF only (classic `liberate --pdf`).
        #[arg(short, long)]
        pdf: bool,
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
        /// Emit full license summary as JSON (classic prints license JSON).
        #[arg(long)]
        json: bool,
    },
    /// Search the library index (LibationCli: `search`).
    Search {
        /// Lucene-style query string.
        query: String,
        /// Max results (0 = all, classic `-n 0`).
        #[arg(short = 'n', long, default_value = "10")]
        limit: usize,
        /// Print ASINs only.
        #[arg(short, long)]
        bare: bool,
        /// Rebuild the search index before querying.
        #[arg(long)]
        rebuild_index: bool,
    },
    /// Export library rows (LibationCli: `export`).
    Export {
        /// Output file path.
        #[arg(short, long)]
        path: std::path::PathBuf,
        #[arg(long)]
        csv: bool,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        xlsx: bool,
        /// Limit to specific ASINs.
        asins: Vec<String>,
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
                "scan complete: {} account(s), {} book upsert(s), {} page(s), {} skipped (scan disabled)",
                summary.accounts, summary.books_upserted, summary.pages, summary.skipped_disabled
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
            let engine = SearchEngine::open(&paths.search_index_dir)?;
            let indexed = engine.rebuild(&store)?;
            println!("search index rebuilt: {indexed} book(s)");
            Ok(())
        }
        LibraryCommand::Liberate {
            asin,
            asins,
            account,
            dry_run,
            force,
            pdf,
        } => {
            let storage = from_config(config).await?;

            // Match existing media first (same as libationd) so we do not
            // re-download titles already on disk.
            if !dry_run {
                let _ = reconcile_library(
                    &store,
                    storage.as_ref(),
                    ReconcileOptions {
                        account: account.clone(),
                        clear_missing: true,
                    },
                )
                .await?;
            }

            let books = store.list_books(account.as_deref())?;
            let filter_asins: Vec<String> = asin
                .into_iter()
                .chain(asins)
                .map(|a| a.to_ascii_uppercase())
                .collect();
            let targets: Vec<_> = books
                .into_iter()
                .filter(|b| {
                    filter_asins.is_empty()
                        || filter_asins
                            .iter()
                            .any(|a| a.eq_ignore_ascii_case(&b.asin))
                })
                .filter(|b| {
                    if pdf {
                        force || b.pdf_status != LiberateStatus::Liberated
                    } else {
                        force || b.liberate_status != LiberateStatus::Liberated
                    }
                })
                .collect();

            if targets.is_empty() {
                eprintln!("nothing to liberate");
                return Ok(());
            }

            paths.ensure_dirs()?;
            let options = DownloadOptions::from(&config.download);
            let mut index = if dry_run {
                None
            } else {
                Some(StorageIndex::from_storage(storage.as_ref()).await?)
            };

            let mut ok = 0u32;
            let mut matched = 0u32;
            let mut failed = 0u32;

            for book in targets {
                let req = LiberateRequest {
                    asin: book.asin.clone(),
                    account_id: book.account_id.clone(),
                    title: book.title.clone(),
                    authors: book.authors.clone(),
                    narrators: book.narrators.clone(),
                    series: book.series.clone(),
                    series_index: book.series_index.clone(),
                    options: options.clone(),
                    files_dir: paths.files_dir.clone(),
                    cache_dir: config
                        .download
                        .in_progress
                        .clone()
                        .unwrap_or_else(|| paths.cache_dir.clone()),
                    aaxclean_bin: None,
                    ffmpeg_bin: None,
                    force,
                };
                if dry_run {
                    let key = if pdf {
                        libation_liberate::sidecar_key(
                            &libation_liberate::planned_storage_key(&req),
                            "pdf",
                        )
                    } else {
                        libation_liberate::planned_storage_key(&req)
                    };
                    println!("{}\t{}", book.asin, key);
                    continue;
                }
                let result = if pdf {
                    liberate_pdf_only(&store, storage.as_ref(), &req).await
                } else {
                    liberate_book_indexed(&store, storage.as_ref(), req, index.as_mut()).await
                };
                match result {
                    Ok(result) if result.matched_existing => {
                        println!("matched {} -> {}", result.asin, result.storage_key);
                        matched += 1;
                    }
                    Ok(result) => {
                        println!("liberated {} -> {}", result.asin, result.storage_key);
                        ok += 1;
                    }
                    Err(err) => {
                        eprintln!("liberate {}: {err}", book.asin);
                        failed += 1;
                    }
                }
            }

            if dry_run {
                return Ok(());
            }
            if failed > 0 {
                anyhow::bail!(
                    "liberate finished with {failed} failure(s) (liberated={ok} matched={matched})"
                );
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
        LibraryCommand::GetLicense { asin, account, json } => {
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
            if json {
                println!("{}", serde_json::to_string_pretty(&summary)?);
                return Ok(());
            }
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
        LibraryCommand::Search {
            query,
            limit,
            bare,
            rebuild_index,
        } => {
            let engine = SearchEngine::open(&paths.search_index_dir)?;
            if rebuild_index {
                let n = engine.rebuild(&store)?;
                println!("search index rebuilt: {n} book(s)");
            }
            let hits = engine.search(&query, limit)?;
            if limit != 0 {
                println!("Found {} matching result(s).", hits.len());
            }
            for hit in hits {
                if bare {
                    println!("{}", hit.asin);
                } else {
                    println!("{} — {}", hit.asin, hit.title);
                }
            }
            Ok(())
        }
        LibraryCommand::Export {
            path,
            csv,
            json,
            xlsx,
            asins,
            account,
        } => {
            let books = filter_books(
                load_books(&store, account.as_deref())?,
                if asins.is_empty() {
                    None
                } else {
                    Some(&asins)
                },
            );
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if csv || (!json && !xlsx && ext == "csv") {
                export_csv(&path, &books)?;
            } else if json || ext == "json" {
                export_json(&path, &books)?;
            } else if xlsx || ext == "xlsx" {
                export_xlsx(&path, &books)?;
            } else {
                export_csv(&path, &books)?;
            }
            println!("exported {} book(s) to {}", books.len(), path.display());
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
