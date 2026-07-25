//! `bookclerk daemon` — thin HTTP client for bookclerkd.

use bookclerk_config::Config;
use clap::Subcommand;
use serde_json::Value;

use crate::format_out::{emit, OutputFormat};

#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// GET /health
    Health,
    /// GET /status
    Status,
    /// POST /scan
    Scan {
        #[arg(long)]
        account: Option<String>,
    },
    /// POST /acquire
    Acquire {
        #[arg(long)]
        asin: Option<String>,
        #[arg(long)]
        account: Option<String>,
    },
    /// GET /jobs
    Jobs,
}

pub async fn run(
    command: DaemonCommand,
    config: &Config,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let base = daemon_base_url(config);
    match command {
        DaemonCommand::Health => {
            let v = get_json(&format!("{base}/health"))?;
            emit(format, &v, || {
                println!("{}", v["status"].as_str().unwrap_or("ok"));
            })
        }
        DaemonCommand::Status => {
            let v = get_json(&format!("{base}/status"))?;
            emit(format, &v, || {
                println!(
                    "accounts={} books={} acquired={} pending={} error={} in_progress={} listen={} storage={}",
                    v["accounts"],
                    v["books"],
                    v["acquired"],
                    v["pending"],
                    v["error"],
                    v["in_progress"],
                    v["listen"].as_str().unwrap_or("-"),
                    v["storage_backend"].as_str().unwrap_or("-"),
                );
            })
        }
        DaemonCommand::Scan { account } => {
            let body = serde_json::json!({ "account": account });
            let v = post_json(&format!("{base}/scan"), &body)?;
            emit(format, &v, || {
                println!(
                    "ok={} job_id={} {}",
                    v["ok"],
                    v["job_id"].as_str().unwrap_or("-"),
                    v["message"].as_str().unwrap_or("")
                );
            })
        }
        DaemonCommand::Acquire { asin, account } => {
            let body = serde_json::json!({ "asin": asin, "account": account });
            let v = post_json(&format!("{base}/acquire"), &body)?;
            emit(format, &v, || {
                println!(
                    "ok={} job_id={} {}",
                    v["ok"],
                    v["job_id"].as_str().unwrap_or("-"),
                    v["message"].as_str().unwrap_or("")
                );
            })
        }
        DaemonCommand::Jobs => {
            let v = get_json(&format!("{base}/jobs"))?;
            emit(format, &v, || {
                let jobs = v.as_array().cloned().unwrap_or_default();
                if jobs.is_empty() {
                    println!("no jobs");
                    return;
                }
                for job in jobs {
                    println!(
                        "{} kind={} status={} {}",
                        job["id"].as_str().unwrap_or("-"),
                        job["kind"].as_str().unwrap_or("-"),
                        job["status"].as_str().unwrap_or("-"),
                        job["detail"].as_str().unwrap_or("")
                    );
                }
            })
        }
    }
}

fn daemon_base_url(config: &Config) -> String {
    let listen = config.daemon.listen.trim();
    if listen.starts_with("http://") || listen.starts_with("https://") {
        listen.trim_end_matches('/').to_string()
    } else {
        format!("http://{listen}")
    }
}

fn get_json(url: &str) -> anyhow::Result<Value> {
    let resp = ureq::get(url)
        .call()
        .map_err(|err| anyhow::anyhow!("daemon GET {url}: {err}"))?;
    resp.into_json()
        .map_err(|err| anyhow::anyhow!("daemon JSON: {err}"))
}

fn post_json(url: &str, body: &Value) -> anyhow::Result<Value> {
    let resp = ureq::post(url)
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|err| anyhow::anyhow!("daemon POST {url}: {err}"))?;
    resp.into_json()
        .map_err(|err| anyhow::anyhow!("daemon JSON: {err}"))
}
