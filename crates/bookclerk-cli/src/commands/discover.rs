//! Discovery: recommendations, embeddings, listening sync, title requests.

use anyhow::{bail, Result};
use bookclerk_config::Config;
use bookclerk_library::{LibraryStore, NewTitleRequest, RequestStatus};
use clap::Subcommand;

use crate::format_out::{self, OutputFormat};

#[derive(Debug, Subcommand)]
pub enum DiscoverCommand {
    /// Rebuild works graph, optionally enrich + embed, then print recommendations.
    Recommend {
        /// Max recommendations to print.
        #[arg(long, short = 'n')]
        limit: Option<usize>,
        /// Skip purchase-hint HTTP lookups.
        #[arg(long)]
        no_purchase_hints: bool,
        /// Filter listening signals to one ABS external user id.
        #[arg(long)]
        user: Option<String>,
    },
    /// Rebuild the works graph from ownership rows.
    RebuildWorks,
    /// Run Open Library enrichment for metadata gaps.
    EnrichOpenlibrary,
    /// Embed dirty works.
    Embed {
        /// Force the local-hash embedder (no ONNX download).
        #[arg(long)]
        hash: bool,
    },
    /// Sync AudioBookshelf listening progress into the library DB.
    SyncListening,
    /// Title request queue.
    Request {
        #[command(subcommand)]
        command: RequestCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum RequestCommand {
    /// Add an open title request.
    Add {
        title: String,
        #[arg(long)]
        authors: Option<String>,
        #[arg(long)]
        asin: Option<String>,
        #[arg(long)]
        isbn: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        preferred_source: Option<String>,
    },
    /// List title requests.
    List {
        #[arg(long, value_parser = ["open", "approved", "acquired", "rejected", "cancelled"])]
        status: Option<String>,
    },
    /// Update request status.
    SetStatus {
        uuid: String,
        #[arg(value_parser = ["open", "approved", "acquired", "rejected", "cancelled"])]
        status: String,
        #[arg(long)]
        resolved_book: Option<String>,
    },
}

pub async fn run(cfg: &Config, format: OutputFormat, command: DiscoverCommand) -> Result<()> {
    let paths = cfg.paths();
    let library = LibraryStore::open(&paths.library_db)?;

    match command {
        DiscoverCommand::RebuildWorks => {
            let n = bookclerk_discover::rebuild_works_from_library(&library)?;
            println!("linked {n} book(s) into works");
        }
        DiscoverCommand::EnrichOpenlibrary => {
            let opts = bookclerk_discover::OpenLibraryOptions {
                contact_email: cfg.discovery.openlibrary_contact_email.clone(),
                max_requests: cfg.discovery.openlibrary_max_requests_per_run.max(1),
                ..Default::default()
            };
            let n = bookclerk_discover::enrich_books_from_openlibrary_with(&library, &opts).await?;
            println!("open library enriched {n} book(s)");
        }
        DiscoverCommand::Embed { hash } => {
            let n = embed_works(cfg, &library, !hash && cfg.discovery.embeddings_enabled)?;
            println!("embedded {n} work(s)");
        }
        DiscoverCommand::SyncListening => {
            let n = sync_listening(cfg, &library).await?;
            println!("upserted {n} listening progress row(s)");
        }
        DiscoverCommand::Recommend {
            limit,
            no_purchase_hints,
            user,
        } => {
            let _ = bookclerk_discover::rebuild_works_from_library(&library)?;
            let prefer_onnx = cfg.discovery.embeddings_enabled;
            let mut embedder = bookclerk_discover::open_embedder(
                &cfg.paths().models_dir,
                cfg.discovery.embed_intra_threads,
                prefer_onnx,
            )?;
            let model_id = embedder.model_id().to_string();
            let _ = bookclerk_discover::embed_dirty_works(&library, embedder.as_mut())?;

            if cfg.discovery.openlibrary_enabled {
                let ol = bookclerk_discover::OpenLibraryOptions {
                    contact_email: cfg.discovery.openlibrary_contact_email.clone(),
                    max_requests: cfg.discovery.openlibrary_max_requests_per_run.max(1),
                    ..Default::default()
                };
                let _ = bookclerk_discover::enrich_books_from_openlibrary_with(&library, &ol).await;
            }

            let opts = bookclerk_discover::RecommendOptions {
                limit: limit.unwrap_or(cfg.discovery.recommend_limit),
                embedding_model: model_id,
                region: String::from("us"),
                include_purchase_hints: !no_purchase_hints,
                external_user_id: user,
                fetch_storefront_candidates: cfg.discovery.storefront_candidates,
                storefront_seed_limit: cfg.discovery.storefront_seed_limit,
                storefront_max_remote_calls: cfg.discovery.storefront_max_remote_calls,
                exclude_graphicaudio_series_sets: cfg.discovery.exclude_graphicaudio_series_sets,
                models_dir: Some(cfg.paths().models_dir.clone()),
                embed_intra_threads: cfg.discovery.embed_intra_threads,
                embeddings_enabled: cfg.discovery.embeddings_enabled,
            };
            let recs = bookclerk_discover::recommend(&library, &opts).await?;
            format_out::emit(format, &recs, || {
                if recs.is_empty() {
                    println!("(no recommendations)");
                }
                for (i, r) in recs.iter().enumerate() {
                    println!(
                        "{}. {} [{}] score={:.2}",
                        i + 1,
                        r.title,
                        r.authors.as_deref().unwrap_or("?"),
                        r.score
                    );
                    if !r.reasons.is_empty() {
                        println!("   reasons: {}", r.reasons.join("; "));
                    }
                    for h in &r.purchase_hints {
                        let url = h
                            .url
                            .as_ref()
                            .map(|u| format!(" ({u})"))
                            .unwrap_or_default();
                        println!("   buy via {}: {}{url}", h.source, h.product_id);
                    }
                }
            })?;
        }
        DiscoverCommand::Request { command } => match command {
            RequestCommand::Add {
                title,
                authors,
                asin,
                isbn,
                notes,
                preferred_source,
            } => {
                let row = library.create_title_request(&NewTitleRequest {
                    uuid: None,
                    identity_id: None,
                    title,
                    authors,
                    asin,
                    isbn,
                    notes,
                    status: RequestStatus::Open,
                    preferred_source,
                    work_id: None,
                    resolved_book_uuid: None,
                })?;
                format_out::emit(format, &row, || {
                    println!("created request {}", row.uuid);
                })?;
            }
            RequestCommand::List { status } => {
                let filter = match status.as_deref() {
                    Some(s) => Some(
                        RequestStatus::parse(s)
                            .ok_or_else(|| anyhow::anyhow!("unknown status {s}"))?,
                    ),
                    None => None,
                };
                let rows = library.list_title_requests(filter)?;
                format_out::emit(format, &rows, || {
                    for r in &rows {
                        println!(
                            "{} [{}] {} — {}",
                            r.uuid,
                            r.status.as_str(),
                            r.title,
                            r.authors.as_deref().unwrap_or("?")
                        );
                    }
                })?;
            }
            RequestCommand::SetStatus {
                uuid,
                status,
                resolved_book,
            } => {
                let st = RequestStatus::parse(&status)
                    .ok_or_else(|| anyhow::anyhow!("unknown status {status}"))?;
                library.update_title_request_status(&uuid, st, resolved_book.as_deref())?;
                println!("updated {uuid} → {}", st.as_str());
            }
        },
    }
    Ok(())
}

fn embed_works(cfg: &Config, library: &LibraryStore, prefer_onnx: bool) -> Result<usize> {
    let mut embedder = bookclerk_discover::open_embedder(
        &cfg.paths().models_dir,
        cfg.discovery.embed_intra_threads,
        prefer_onnx,
    )?;
    Ok(bookclerk_discover::embed_dirty_works(
        library,
        embedder.as_mut(),
    )?)
}

async fn sync_listening(cfg: &Config, library: &LibraryStore) -> Result<usize> {
    let abs = &cfg.integrations.audiobookshelf;
    if !abs.enabled {
        bail!("integrations.audiobookshelf is disabled");
    }
    let base = abs.base_url.trim();
    let key = abs.api_key.as_deref().unwrap_or("").trim();
    if base.is_empty() || key.is_empty() {
        bail!("integrations.audiobookshelf.base_url and api_key are required");
    }
    let client = bookclerk_integrations::abs::AbsApiClient::new(base, key)?;
    Ok(bookclerk_integrations::abs::sync_listening_progress(library, &client).await?)
}
