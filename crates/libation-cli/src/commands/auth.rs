//! `libation auth` — login, import, list, status.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Subcommand;
use libation_audible::{
    begin_login, import_libation_accounts_json, list_accounts_stub, AuthLoginOptions,
    LoginProgress, QrRenderMode,
};
use libation_config::Config;
use libation_library::LibraryStore;

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Log in via browser / QR (LibationCli: `login-external`).
    Login {
        /// Marketplace code (`us`, `uk`, `de`, …).
        #[arg(short = 'm', long, default_value = "us")]
        marketplace: String,
        /// Optional account label.
        #[arg(long)]
        label: Option<String>,
        /// Amazon redirect URL for non-interactive / Docker use.
        #[arg(long)]
        response_url: Option<String>,
        /// Callback server bind address.
        #[arg(long, default_value = "127.0.0.1:0")]
        callback_bind: SocketAddr,
        /// Disable terminal QR output.
        #[arg(long)]
        no_qr: bool,
        /// Use ASCII QR instead of Unicode blocks.
        #[arg(long)]
        ascii_qr: bool,
    },
    /// Import audible auth file or Libation AccountsSettings.json.
    Import {
        /// Path to auth file or AccountsSettings.json.
        path: PathBuf,
        /// Treat input as Libation AccountsSettings.json.
        #[arg(long)]
        libation_accounts: bool,
    },
    /// List configured accounts.
    List,
    /// Show token validity / refresh health.
    Status,
}

pub async fn run(command: AuthCommand, config: &Config) -> anyhow::Result<()> {
    let paths = config.paths();
    match command {
        AuthCommand::Login {
            marketplace,
            label,
            response_url,
            callback_bind,
            no_qr,
            ascii_qr,
        } => {
            let opts = AuthLoginOptions {
                marketplace,
                label,
                callback_bind,
                response_url,
                show_qr: !no_qr,
                qr_mode: if ascii_qr {
                    QrRenderMode::Ascii
                } else {
                    QrRenderMode::Unicode
                },
                files_dir: paths.files_dir.clone(),
            };

            match begin_login(opts, |progress| match progress {
                LoginProgress::LoginUrl { url, qr } => {
                    if let Some(qr) = qr {
                        println!("{qr}");
                    } else {
                        println!("{url}");
                    }
                }
                LoginProgress::CallbackListening { addr } => {
                    eprintln!("callback server ready on {addr}");
                }
                LoginProgress::WaitingForCallback => {
                    eprintln!("waiting for browser callback (or pass --response-url)…");
                }
                LoginProgress::Completed { account_id } => {
                    eprintln!("login completed for {account_id}");
                }
            })
            .await
            {
                Ok(session) => {
                    let store = LibraryStore::open(&paths.library_db).await?;
                    store
                        .upsert_account(
                            &session.account_id,
                            &session.marketplace,
                            session.label.as_deref(),
                            true,
                        )
                        .await?;
                    println!(
                        "authenticated {} ({})",
                        session.account_id, session.marketplace
                    );
                    Ok(())
                }
                Err(err) => Err(err.into()),
            }
        }
        AuthCommand::Import {
            path,
            libation_accounts,
        } => {
            if libation_accounts || path.ends_with("AccountsSettings.json") {
                let accounts = import_libation_accounts_json(&path)?;
                let store = LibraryStore::open(&paths.library_db).await?;
                for acct in &accounts {
                    store
                        .upsert_account(
                            &acct.account_id,
                            &acct.marketplace,
                            acct.label.as_deref(),
                            true,
                        )
                        .await?;
                    println!("imported {} ({})", acct.account_id, acct.marketplace);
                }
                if accounts.is_empty() {
                    eprintln!("no accounts found in {}", path.display());
                }
                Ok(())
            } else {
                anyhow::bail!(
                    "audible auth-file import not wired yet; pass --libation-accounts for \
                     AccountsSettings.json"
                );
            }
        }
        AuthCommand::List => {
            let store = LibraryStore::open(&paths.library_db).await?;
            let db_accounts = store.list_accounts().await?;
            let stub = list_accounts_stub(&paths.files_dir)?;
            if db_accounts.is_empty() && stub.is_empty() {
                eprintln!("no accounts configured — run `libation auth login`");
                return Ok(());
            }
            for acct in db_accounts {
                println!(
                    "{}\t{}\t{}\tscan={}",
                    acct.account_id,
                    acct.marketplace,
                    acct.label.as_deref().unwrap_or("-"),
                    acct.scan_enabled
                );
            }
            Ok(())
        }
        AuthCommand::Status => {
            let store = LibraryStore::open(&paths.library_db).await?;
            let accounts = store.list_accounts().await?;
            if accounts.is_empty() {
                eprintln!("no accounts configured");
                return Ok(());
            }
            for acct in accounts {
                // Token health will come from audible-rs; scaffold reports presence only.
                println!(
                    "{}\t{}\tstatus=unknown (token probe pending)",
                    acct.account_id, acct.marketplace
                );
            }
            Ok(())
        }
    }
}
