//! `libation auth` — login, import, list, status.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Subcommand;
use libation_audible::{
    begin_login, import_auth_file, import_libation_accounts_json, import_mkb79_auth_json,
    list_accounts, AuthLoginOptions, LoginMode, LoginProgress, QrRenderMode,
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
        /// Optional account label (also used as auth filename stem).
        #[arg(long)]
        label: Option<String>,
        /// Print authorize URL and paste redirect (instead of local login server).
        #[arg(long)]
        external: bool,
        /// Amazon redirect URL (with `--external`; otherwise read from stdin).
        #[arg(long)]
        response_url: Option<String>,
        /// Callback server bind address (login-server mode).
        #[arg(long, default_value = "127.0.0.1:0")]
        callback_bind: SocketAddr,
        /// Seconds to wait for login-server capture.
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        /// Pre-merger Audible username login (de/us/uk only).
        #[arg(long)]
        audible_username: bool,
        /// Overwrite an existing auth file.
        #[arg(long)]
        force: bool,
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
        /// Import mkb79/audible-cli legacy auth JSON (classic `import-account`).
        #[arg(long)]
        mkb79: bool,
        /// Destination auth filename stem (auth-file import).
        #[arg(long)]
        label: Option<String>,
        /// Overwrite an existing auth file.
        #[arg(long)]
        force: bool,
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
            external,
            response_url,
            callback_bind,
            timeout,
            audible_username,
            force,
            no_qr,
            ascii_qr,
        } => {
            let mode = if external || response_url.is_some() {
                LoginMode::External
            } else {
                LoginMode::Server
            };
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
                mode,
                timeout_secs: timeout,
                audible_username,
                force,
            };

            let session = begin_login(opts, |progress| match progress {
                LoginProgress::LoginUrl { url, qr } => {
                    if let Some(qr) = qr {
                        println!("{qr}");
                    } else {
                        println!("{url}");
                    }
                }
                LoginProgress::CallbackListening { addr } => {
                    eprintln!("callback server listening on {addr}");
                    if addr.ip().is_loopback() {
                        eprintln!(
                            "On a remote host, forward the port: ssh -L {port}:localhost:{port} user@host",
                            port = addr.port()
                        );
                    }
                }
                LoginProgress::WaitingForCallback => {
                    eprintln!("waiting for browser sign-in…");
                }
                LoginProgress::Completed { account_id } => {
                    eprintln!("login completed for {account_id}");
                }
            })
            .await?;

            let store = LibraryStore::open(&paths.library_db)?;
            // Remap classic email AccountId / auth-file stem onto customer_id.
            if let Some(label) = session.label.as_deref() {
                if label != session.account_id {
                    let _ = store.remap_account_id(label, &session.account_id);
                }
            }
            store.upsert_account(
                &session.account_id,
                &session.marketplace,
                session.label.as_deref(),
                true,
            )?;
            println!(
                "authenticated {} ({}) → {}",
                session.account_id,
                session.marketplace,
                session.auth_file.display()
            );
            Ok(())
        }
        AuthCommand::Import {
            path,
            libation_accounts,
            mkb79,
            label,
            force,
        } => {
            if libation_accounts || path.ends_with("AccountsSettings.json") {
                let accounts = import_libation_accounts_json(&path)?;
                let store = LibraryStore::open(&paths.library_db)?;
                for acct in &accounts {
                    store.upsert_account(
                        &acct.account_id,
                        &acct.marketplace,
                        acct.label.as_deref(),
                        true,
                    )?;
                    println!("imported {} ({})", acct.account_id, acct.marketplace);
                }
                if accounts.is_empty() {
                    eprintln!("no accounts found in {}", path.display());
                }
                Ok(())
            } else if mkb79 {
                let acct =
                    import_mkb79_auth_json(&paths.files_dir, &path, label.as_deref(), force)
                        .await?;
                let store = LibraryStore::open(&paths.library_db)?;
                store.upsert_account(
                    &acct.account_id,
                    &acct.marketplace,
                    acct.label.as_deref(),
                    true,
                )?;
                println!(
                    "imported mkb79 account {} ({}) → {}",
                    acct.account_id,
                    acct.marketplace,
                    acct.auth_file.as_deref().unwrap_or("-")
                );
                Ok(())
            } else {
                let acct = import_auth_file(&paths.files_dir, &path, label.as_deref(), force).await?;
                let store = LibraryStore::open(&paths.library_db)?;
                store.upsert_account(
                    &acct.account_id,
                    &acct.marketplace,
                    acct.label.as_deref(),
                    true,
                )?;
                println!(
                    "imported auth {} ({}) → {}",
                    acct.account_id,
                    acct.marketplace,
                    acct.auth_file.as_deref().unwrap_or("-")
                );
                Ok(())
            }
        }
        AuthCommand::List => {
            let accounts = list_accounts(&paths.files_dir).await?;
            let store = LibraryStore::open(&paths.library_db)?;
            let db_accounts = store.list_accounts()?;
            if accounts.is_empty() && db_accounts.is_empty() {
                eprintln!("no accounts configured — run `libation auth login`");
                return Ok(());
            }
            for acct in &accounts {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    acct.account_id,
                    acct.marketplace,
                    acct.label.as_deref().unwrap_or("-"),
                    acct.status.as_str(),
                    acct.auth_file.as_deref().unwrap_or("-")
                );
            }
            let auth_ids: std::collections::HashSet<_> =
                accounts.iter().map(|a| a.account_id.as_str()).collect();
            for db in db_accounts {
                if auth_ids.contains(db.account_id.as_str()) {
                    continue;
                }
                println!(
                    "{}\t{}\t{}\tdb_only\t-",
                    db.account_id,
                    db.marketplace,
                    db.label.as_deref().unwrap_or("-")
                );
            }
            Ok(())
        }
        AuthCommand::Status => {
            let accounts = list_accounts(&paths.files_dir).await?;
            if accounts.is_empty() {
                eprintln!("no accounts configured");
                return Ok(());
            }
            for acct in accounts {
                println!(
                    "{}\t{}\tstatus={}",
                    acct.account_id,
                    acct.marketplace,
                    acct.status.as_str()
                );
            }
            Ok(())
        }
    }
}
