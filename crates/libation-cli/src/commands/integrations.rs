//! `libation integrations` — status, test, tickets, ABS scan.

use clap::Subcommand;
use libation_config::Config;
use libation_integrations::{mint_for_external_user, ExternalUser};
use libation_library::LibraryStore;

#[derive(Debug, Subcommand)]
pub enum IntegrationsCommand {
    /// Show health of configured integrations.
    Status,
    /// Authorize against Audiobookshelf and list libraries.
    Test,
    /// Claim ticket management.
    Tickets {
        #[command(subcommand)]
        command: TicketsCommand,
    },
    /// Trigger an Audiobookshelf library scan.
    AbsScan {
        /// Force full rescan.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum TicketsCommand {
    /// Mint a claim ticket for an external identity.
    Create {
        /// Integration provider id (`audiobookshelf`).
        #[arg(long, default_value = "audiobookshelf")]
        provider: String,
        /// External user id from the provider.
        #[arg(long)]
        external_user_id: String,
        /// Optional display label.
        #[arg(long)]
        label: Option<String>,
    },
    /// List open (unredeemed, unexpired) claim tickets.
    List,
}

pub async fn run(command: IntegrationsCommand, config: &Config) -> anyhow::Result<()> {
    let paths = config.paths().clone();
    paths.ensure_dirs()?;
    let library = LibraryStore::open(&paths.library_db)?;
    let registry = libation_integrations::from_config(config)?;

    match command {
        IntegrationsCommand::Status => {
            let health = registry.health_all().await;
            if health.is_empty() {
                println!("no integrations enabled");
            }
            for h in health {
                println!(
                    "{} enabled={} ok={} {}",
                    h.id,
                    h.enabled,
                    h.ok,
                    h.detail.unwrap_or_default()
                );
            }
            Ok(())
        }
        IntegrationsCommand::Test => {
            let abs = config.integrations.audiobookshelf.clone();
            if !abs.enabled {
                anyhow::bail!("integrations.audiobookshelf.enabled is false");
            }
            let key = abs
                .api_key
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("LIBATION_ABS_API_KEY / api_key required"))?;
            let client = libation_integrations::AbsApiClient::new(&abs.base_url, key)?;
            let auth = client.authorize().await?;
            if let Some(user) = auth.user {
                println!("authorized as {} ({})", user.username, user.id);
            } else {
                println!("authorized (no user in response)");
            }
            let libs = client.list_libraries().await?;
            for lib in libs {
                println!("library {} — {}", lib.id, lib.name);
            }
            Ok(())
        }
        IntegrationsCommand::Tickets { command } => match command {
            TicketsCommand::Create {
                provider,
                external_user_id,
                label,
            } => {
                let user = ExternalUser {
                    provider,
                    external_user_id,
                    display_name: label,
                    access_token: None,
                };
                let minted = mint_for_external_user(&library, config, &user, "cli")?;
                println!("ticket={}", minted.token);
                if let Some(url) = minted.portal_url {
                    println!("url={url}");
                }
                println!(
                    "identity={} expires={}",
                    minted.identity.id,
                    minted.record.expires_at.to_rfc3339()
                );
                Ok(())
            }
            TicketsCommand::List => {
                let tickets = library.list_open_claim_tickets()?;
                if tickets.is_empty() {
                    println!("no open claim tickets");
                }
                for t in tickets {
                    println!(
                        "id={} identity={:?} expires={} created_by={} hash={}…",
                        t.id,
                        t.identity_id,
                        t.expires_at.to_rfc3339(),
                        t.created_by,
                        &t.token_hash[..8.min(t.token_hash.len())]
                    );
                }
                Ok(())
            }
        },
        IntegrationsCommand::AbsScan { force } => {
            let abs = &config.integrations.audiobookshelf;
            let key = abs
                .api_key
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("api_key required"))?;
            let library_id = abs
                .library_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("library_id required"))?;
            let client = libation_integrations::AbsApiClient::new(&abs.base_url, key)?;
            client.scan_library(library_id, force).await?;
            println!("scan started for {library_id}");
            Ok(())
        }
    }
}
